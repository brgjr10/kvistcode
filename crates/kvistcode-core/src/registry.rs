//! The official ACP Registry (https://github.com/agentclientprotocol/registry): a public
//! index of Agent Client Protocol agents. Everything in it is third-party data, so each entry
//! is checked here before it can become a custom agent. Nothing is cached or polled: the index
//! is fetched when the person opens the browser and again when they add an agent.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use serde::Serialize;
use serde_json::Value;

use crate::model::{CustomAgent, KeyValue};
use crate::toolchain;
use crate::Result;

pub const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// The index is about 80 KB today; anything far beyond that isn't the registry.
const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;

/// How an agent from the registry is started, after validation.
#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// `npx -y <package> <args>`, run on Kvistcode's or the person's Node.js.
    Npx {
        package: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// `uvx <package> <args>`; needs `uv` on the person's computer.
    Uvx {
        package: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// A download for this platform. The SHA-256 is required.
    Binary {
        archive: String,
        sha256: String,
        cmd: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
}

/// One registry agent as the UI shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryAgent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub icon: Option<String>,
    pub website: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    /// "npx", "uvx" or "binary"; empty when it can't be added.
    pub kind: String,
    /// Plain-text summary of what will run on the computer.
    pub runs: String,
    /// Where the code comes from (npm package, PyPI package or download host).
    pub source: String,
    /// Why it can't be added here, when it can't.
    pub unsupported: Option<String>,
}

/// An entry plus the checked launch plan behind it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub info: RegistryAgent,
    pub launch: Option<Launch>,
}

/// The registry's key for this computer's platform, if it publishes builds for it.
pub fn platform_key() -> Option<&'static str> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        _ => return None,
    };
    Some(match (os, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("darwin", "x86_64") => "darwin-x86_64",
        ("darwin", "aarch64") => "darwin-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        _ => return None,
    })
}

// ---------------------------------------------------------------- validation

fn clean_text(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// An https URL with a host and nothing sneaky in it.
fn https_url(u: &str) -> Option<&str> {
    let rest = u.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    let ok = u.len() <= 500
        && !host.is_empty()
        && !host.contains(['@', ' ', '\\'])
        && !u.chars().any(|c| c.is_control() || c.is_whitespace());
    ok.then_some(host)
}

/// An argument handed to the agent: one short line, no control characters.
fn safe_arg(a: &str) -> bool {
    !a.is_empty() && a.len() <= 200 && !a.chars().any(|c| c.is_control())
}

fn safe_args(v: Option<&Value>) -> Option<Vec<String>> {
    let Some(v) = v else {
        return Some(Vec::new());
    };
    let arr = v.as_array().filter(|a| a.len() <= 20)?;
    arr.iter()
        .map(|a| a.as_str().filter(|s| safe_arg(s)).map(str::to_string))
        .collect()
}

/// Variables that would change how the agent's launcher itself behaves.
fn risky_env(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    matches!(
        up.as_str(),
        "PATH" | "PATHEXT" | "HOME" | "USERPROFILE" | "SHELL" | "COMSPEC" | "TMPDIR" | "TEMP"
    ) || [
        "LD_", "DYLD_", "NODE_", "NPM_", "PYTHON", "UV_", "BASH_", "GIT_",
    ]
    .iter()
    .any(|p| up.starts_with(p))
}

fn safe_env(v: Option<&Value>) -> Option<Vec<(String, String)>> {
    let Some(v) = v else {
        return Some(Vec::new());
    };
    let obj = v.as_object().filter(|o| o.len() <= 20)?;
    obj.iter()
        .map(|(k, val)| {
            let name_ok = !k.is_empty()
                && k.len() <= 64
                && !k.starts_with(|c: char| c.is_ascii_digit())
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !risky_env(k);
            let val = val
                .as_str()
                .filter(|s| s.len() <= 500 && !s.chars().any(|c| c.is_control()))?;
            name_ok.then(|| (k.clone(), val.to_string()))
        })
        .collect()
}

/// `name@1.2.3` or `@scope/name@1.2.3`. A pinned version is required: "latest" would run
/// whatever was published last.
fn npm_package(p: &str) -> bool {
    let Some(at) = p.rfind('@').filter(|i| *i > 0) else {
        return false;
    };
    let (name, version) = (&p[..at], &p[at + 1..]);
    let part = |s: &str| {
        !s.is_empty()
            && s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._~-".contains(c))
    };
    let name_ok = match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(a, b)| part(a) && part(b)),
        None => part(name),
    };
    name_ok && p.len() <= 120 && version_ok(version)
}

fn version_ok(v: &str) -> bool {
    v.starts_with(|c: char| c.is_ascii_digit())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || ".+-_!".contains(c))
}

/// `name==1.2.3`, pinned like the npm packages.
fn pypi_package(p: &str) -> bool {
    let Some((name, version)) = p.split_once("==") else {
        return false;
    };
    name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        && p.len() <= 120
        && version_ok(version)
}

/// The executable's path inside an archive: relative, no `..`, no drive letters.
fn archive_cmd(c: &str) -> Option<PathBuf> {
    if !safe_arg(c) || c.contains(['\\', ':']) {
        return None;
    }
    let path = Path::new(c.strip_prefix("./").unwrap_or(c));
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(p) => out.push(p),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn package_launch(dist: &Value, kind: &str) -> std::result::Result<Launch, String> {
    let bad = || format!("Its {kind} entry in the registry looks unsafe, so Kvistcode skipped it.");
    let obj = dist.as_object().ok_or_else(bad)?;
    let package = obj.get("package").and_then(Value::as_str).ok_or_else(bad)?;
    let ok = if kind == "npx" {
        npm_package(package)
    } else {
        pypi_package(package)
    };
    if !ok {
        return Err(bad());
    }
    let args = safe_args(obj.get("args")).ok_or_else(bad)?;
    let env = safe_env(obj.get("env")).ok_or_else(bad)?;
    let package = package.to_string();
    Ok(if kind == "npx" {
        Launch::Npx { package, args, env }
    } else {
        Launch::Uvx { package, args, env }
    })
}

fn binary_launch(dist: &Value, platform: Option<&str>) -> std::result::Result<Launch, String> {
    let platform = platform.ok_or("There's no build for this computer.")?;
    let target = dist
        .get(platform)
        .and_then(Value::as_object)
        .ok_or("There's no build for this computer.")?;
    let bad = "Its download entry in the registry looks unsafe, so Kvistcode skipped it.";
    let archive = target.get("archive").and_then(Value::as_str).ok_or(bad)?;
    https_url(archive).ok_or(bad)?;
    let path = archive.split(['?', '#']).next().unwrap_or(archive);
    if !(path.ends_with(".tar.gz") || path.ends_with(".tgz") || path.ends_with(".zip"))
        || archive.contains(['?', '#'])
    {
        return Err(
            "Its download isn't a .zip or .tar.gz file, which Kvistcode can't unpack.".into(),
        );
    }
    let Some(sha) = target.get("sha256").and_then(Value::as_str) else {
        return Err("Its download has no checksum, so Kvistcode can't verify it.".into());
    };
    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad.into());
    }
    let cmd = target
        .get("cmd")
        .and_then(Value::as_str)
        .and_then(archive_cmd)
        .ok_or(bad)?;
    let args = safe_args(target.get("args")).ok_or(bad)?;
    let env = safe_env(target.get("env")).ok_or(bad)?;
    Ok(Launch::Binary {
        archive: archive.to_string(),
        sha256: sha.to_ascii_lowercase(),
        cmd,
        args,
        env,
    })
}

fn describe(launch: &Launch) -> (String, String, String) {
    let join = |args: &[String]| {
        args.iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    };
    match launch {
        Launch::Npx { package, args, .. } => (
            "npx".into(),
            format!("npx -y {package} {}", join(args))
                .trim()
                .to_string(),
            format!("npm package {package}"),
        ),
        Launch::Uvx { package, args, .. } => (
            "uvx".into(),
            format!("uvx {package} {}", join(args)).trim().to_string(),
            format!("PyPI package {package}"),
        ),
        Launch::Binary {
            archive, cmd, args, ..
        } => (
            "binary".into(),
            format!(
                "Downloads {archive}, checks its checksum, then runs {} {}",
                cmd.display(),
                join(args)
            )
            .trim()
            .to_string(),
            format!("download from {}", https_url(archive).unwrap_or("?")),
        ),
    }
}

/// Parses the registry index. Entries without an id or name are dropped; entries that can't
/// be added here stay in the list with the reason in `unsupported`. `has_uvx` says whether
/// `uvx` is on this computer; `platform` is this computer's key in the registry.
pub fn parse_index(json: &str, platform: Option<&str>, has_uvx: bool) -> Result<Vec<Entry>> {
    let root: Value = serde_json::from_str(json).context("the registry isn't valid JSON")?;
    let agents = root
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("the registry has no agent list"))?;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for a in agents {
        let text = |k: &str| a.get(k).and_then(Value::as_str);
        let (Some(id), Some(name)) = (text("id"), text("name")) else {
            continue;
        };
        let name = clean_text(name, 60);
        if !valid_id(id) || name.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        let version = text("version")
            .map(|v| clean_text(v, 40))
            .unwrap_or_default();
        let link = |k: &str| {
            text(k)
                .filter(|u| https_url(u).is_some())
                .map(str::to_string)
        };
        let mut info = RegistryAgent {
            id: id.to_string(),
            name,
            description: clean_text(text("description").unwrap_or(""), 300),
            version,
            icon: link("icon"),
            website: link("website"),
            repository: link("repository"),
            license: text("license")
                .map(|l| clean_text(l, 40))
                .filter(|l| !l.is_empty()),
            kind: String::new(),
            runs: String::new(),
            source: String::new(),
            unsupported: None,
        };
        let dist = a.get("distribution");
        let mut why = String::from("It has no way to start that Kvistcode supports.");
        let mut launch = None;
        // npx first (Kvistcode manages Node.js), then uvx, then a checked download.
        if let Some(d) = dist.and_then(|d| d.get("npx")) {
            match package_launch(d, "npx") {
                Ok(l) => launch = Some(l),
                Err(e) => why = e,
            }
        }
        if launch.is_none() {
            if let Some(d) = dist.and_then(|d| d.get("uvx")) {
                match package_launch(d, "uvx") {
                    Ok(_) if !has_uvx => {
                        why = "It needs uv (uvx), which isn't installed on this computer.".into()
                    }
                    Ok(l) => launch = Some(l),
                    Err(e) => why = e,
                }
            }
        }
        if launch.is_none() {
            if let Some(d) = dist.and_then(|d| d.get("binary")) {
                match binary_launch(d, platform) {
                    Ok(l) => launch = Some(l),
                    Err(e) => why = e,
                }
            }
        }
        match &launch {
            Some(l) => (info.kind, info.runs, info.source) = describe(l),
            None => info.unsupported = Some(why),
        }
        out.push(Entry { info, launch });
    }
    Ok(out)
}

// ---------------------------------------------------------------- network

/// Downloads the index. Blocking; call from `spawn_blocking`.
fn fetch_index() -> Result<String> {
    let mut body = toolchain::http()
        .get(REGISTRY_URL)
        .call()
        .map_err(|_| {
            anyhow!("Couldn't reach the ACP Registry. Check your connection and try again.")
        })?
        .into_body()
        .into_reader()
        .take(MAX_INDEX_BYTES + 1);
    let mut text = String::new();
    body.read_to_string(&mut text)
        .map_err(|_| anyhow!("Couldn't read the ACP Registry's answer. Try again in a moment."))?;
    if text.len() as u64 > MAX_INDEX_BYTES {
        bail!("The ACP Registry's answer was too large, so Kvistcode ignored it.");
    }
    Ok(text)
}

async fn fetch_entries() -> Result<Vec<Entry>> {
    let text = tokio::task::spawn_blocking(fetch_index)
        .await
        .map_err(|_| anyhow!("Couldn't reach the ACP Registry."))??;
    parse_index(&text, platform_key(), which::which("uvx").is_ok())
        .map_err(|_| anyhow!("The ACP Registry's answer wasn't in the expected format."))
}

/// Every agent in the registry, for the browser.
pub async fn list() -> Result<Vec<RegistryAgent>> {
    Ok(fetch_entries().await?.into_iter().map(|e| e.info).collect())
}

/// Sets up what the agent needs (Node.js, or the checked download) and returns the custom
/// agent to save (empty id: the caller picks it). `version` is the one the person saw; if the
/// registry has moved on, nothing is installed.
pub async fn prepare(data_dir: &Path, id: &str, version: &str) -> Result<CustomAgent> {
    let entry = fetch_entries()
        .await?
        .into_iter()
        .find(|e| e.info.id == id)
        .ok_or_else(|| anyhow!("That agent is no longer in the ACP Registry."))?;
    if entry.info.version != version {
        bail!(
            "The registry has a newer version of {}. Close this and open it again.",
            entry.info.name
        );
    }
    if let Some(why) = &entry.info.unsupported {
        bail!("{} can't be added: {why}", entry.info.name);
    }
    let launch = entry
        .launch
        .ok_or_else(|| anyhow!("{} can't be added.", entry.info.name))?;
    let log: toolchain::Log = std::sync::Arc::new(|m: &str| tracing::info!("{m}"));
    let (command, args, env) = match launch {
        Launch::Npx { package, args, env } => {
            toolchain::ensure_node(data_dir, log).await?;
            let mut all = vec!["-y".to_string(), package];
            all.extend(args);
            ("npx".to_string(), all, env)
        }
        Launch::Uvx { package, args, env } => {
            let mut all = vec![package];
            all.extend(args);
            ("uvx".to_string(), all, env)
        }
        Launch::Binary {
            archive,
            sha256,
            cmd,
            args,
            env,
        } => {
            let path =
                toolchain::install_registry_binary(data_dir, id, &archive, &sha256, &cmd, log)
                    .await
                    .map_err(|e| anyhow!("Couldn't install {}: {e}", entry.info.name))?;
            (path.to_string_lossy().into_owned(), args, env)
        }
    };
    Ok(CustomAgent {
        id: String::new(),
        name: entry.info.name,
        command,
        args,
        env: env
            .into_iter()
            .map(|(name, value)| KeyValue { name, value })
            .collect(),
        color: "#B39DDB".into(),
        enabled: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "ca3315e3e54ea85c7e5db09d19a1908cd2ea149c8e83a728cefb814c7a490edf";

    fn index(agents: Value) -> String {
        json!({ "version": "1.0.0", "agents": agents }).to_string()
    }

    fn one(agent: Value, has_uvx: bool) -> Entry {
        let mut v = parse_index(&index(json!([agent])), Some("linux-x86_64"), has_uvx).unwrap();
        assert_eq!(v.len(), 1);
        v.remove(0)
    }

    fn npx(package: &str, args: Value, env: Value) -> Value {
        json!({ "id": "a", "name": "A", "version": "1.0.0",
            "distribution": { "npx": { "package": package, "args": args, "env": env } } })
    }

    fn binary(target: Value) -> Value {
        json!({ "id": "b", "name": "B", "version": "1.0.0",
            "distribution": { "binary": { "linux-x86_64": target } } })
    }

    #[test]
    fn parses_a_package_entry() {
        let e = one(
            json!({ "id": "qwen-code", "name": "Qwen Code", "version": "0.28.4",
                "description": "Coding agent", "icon": "https://cdn.example.com/q.svg",
                "website": "http://insecure.example.com",
                "distribution": { "npx": { "package": "@qwen-code/qwen-code@0.28.4",
                    "args": ["--acp"], "env": { "AUTO_UPDATE": "0" } } } }),
            false,
        );
        assert_eq!(e.info.kind, "npx");
        assert_eq!(e.info.runs, "npx -y @qwen-code/qwen-code@0.28.4 --acp");
        assert_eq!(
            e.info.icon.as_deref(),
            Some("https://cdn.example.com/q.svg")
        );
        assert_eq!(e.info.website, None, "plain http links are dropped");
        assert!(e.info.unsupported.is_none());
        assert!(matches!(e.launch, Some(Launch::Npx { ref env, .. }) if env.len() == 1));
    }

    #[test]
    fn skips_malformed_entries_and_duplicates() {
        let json = index(json!([
            "nope",
            { "name": "No id" },
            { "id": "no-name" },
            { "id": "Bad Id!", "name": "Bad" },
            { "id": "x", "name": "   " },
            npx("ok@1.0.0", json!([]), json!({})),
            npx("other@1.0.0", json!([]), json!({})),
        ]));
        let v = parse_index(&json, None, false).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].info.id, "a");
        assert!(parse_index("not json", None, false).is_err());
        assert!(parse_index("{}", None, false).is_err());
    }

    #[test]
    fn rejects_unsafe_packages_args_and_env() {
        for package in [
            "pkg",
            "pkg@latest",
            "pkg@^1.0.0",
            "-rf@1.0.0",
            "https://evil.example/x.tgz@1.0.0",
            "git+https://example.com/x.git@1.0.0",
            "pkg@1.0.0; rm -rf ~",
            "../x@1.0.0",
            "@scope@1.0.0",
        ] {
            let e = one(npx(package, json!([]), json!({})), false);
            assert!(e.launch.is_none(), "{package} should be rejected");
            assert!(e.info.unsupported.is_some());
        }
        for args in [
            json!([""]),
            json!(["a\nb"]),
            json!([1]),
            json!("--acp"),
            json!(["x".repeat(300)]),
        ] {
            assert!(
                one(npx("ok@1.0.0", args.clone(), json!({})), false)
                    .launch
                    .is_none(),
                "{args}"
            );
        }
        for env in [
            json!({ "PATH": "/tmp/evil" }),
            json!({ "node_options": "--require x" }),
            json!({ "LD_PRELOAD": "/x.so" }),
            json!({ "1BAD": "x" }),
            json!({ "A-B": "x" }),
            json!({ "OK": "a\u{0}b" }),
            json!({ "OK": 1 }),
        ] {
            assert!(
                one(npx("ok@1.0.0", json!([]), env.clone()), false)
                    .launch
                    .is_none(),
                "{env}"
            );
        }
        assert!(one(
            npx(
                "@s/ok-pkg@1.0.0-beta.1",
                json!(["--acp"]),
                json!({ "API_MODE": "1" })
            ),
            false
        )
        .launch
        .is_some());
    }

    #[test]
    fn uvx_needs_a_pinned_package_and_uv_installed() {
        let uvx = |p: &str| json!({ "id": "u", "name": "U", "distribution": { "uvx": { "package": p } } });
        let e = one(uvx("fast-agent-acp==0.10.66"), true);
        assert_eq!(e.info.runs, "uvx fast-agent-acp==0.10.66");
        let e = one(uvx("fast-agent-acp==0.10.66"), false);
        assert!(e.launch.is_none());
        assert!(e.info.unsupported.unwrap().contains("uv"));
        assert!(one(uvx("fast-agent-acp"), true).launch.is_none());
        assert!(one(uvx("git+https://example.com/x==1"), true)
            .launch
            .is_none());
    }

    #[test]
    fn binaries_need_a_checksum_and_a_safe_command() {
        let good = json!({ "archive": "https://github.com/o/r/releases/download/v1/a-linux.tar.gz",
            "cmd": "./a", "args": ["acp"], "sha256": SHA });
        let e = one(binary(good.clone()), false);
        assert_eq!(e.info.kind, "binary");
        assert!(e.info.source.contains("github.com"));
        assert!(matches!(e.launch, Some(Launch::Binary { ref cmd, .. }) if cmd == Path::new("a")));

        let without = |k: &str| {
            let mut t = good.clone();
            t.as_object_mut().unwrap().remove(k);
            t
        };
        let e = one(binary(without("sha256")), false);
        assert!(e.launch.is_none());
        assert!(e.info.unsupported.unwrap().contains("checksum"));
        assert!(one(binary(without("cmd")), false).launch.is_none());
        assert!(one(binary(without("archive")), false).launch.is_none());

        let with = |k: &str, v: Value| {
            let mut t = good.clone();
            t[k] = v;
            t
        };
        for (k, v) in [
            ("sha256", json!("abc123")),
            ("sha256", json!("z".repeat(64))),
            ("cmd", json!("../evil")),
            ("cmd", json!("/usr/bin/evil")),
            ("cmd", json!("a/../../b")),
            ("cmd", json!("C:\\x.exe")),
            ("cmd", json!("")),
            ("archive", json!("http://example.com/a.tar.gz")),
            ("archive", json!("https://user@example.com/a.tar.gz")),
            ("archive", json!("https://example.com/a.tar.gz?x=1")),
            ("archive", json!("https://example.com/a.tar.bz2")),
            ("archive", json!("https://example.com/a.exe")),
        ] {
            assert!(
                one(binary(with(k, v.clone())), false).launch.is_none(),
                "{k}={v}"
            );
        }
        // Nothing published for this platform.
        let v = parse_index(&index(json!([binary(good)])), Some("darwin-aarch64"), false).unwrap();
        assert!(v[0]
            .info
            .unsupported
            .as_ref()
            .unwrap()
            .contains("this computer"));
    }

    #[test]
    fn prefers_npx_then_uvx_then_download() {
        let bin = json!({ "archive": "https://example.com/a.zip", "cmd": "a", "sha256": SHA });
        let e = one(
            json!({ "id": "m", "name": "M", "distribution": {
                "binary": { "linux-x86_64": bin },
                "uvx": { "package": "m==1.0" },
                "npx": { "package": "m@1.0.0" } } }),
            true,
        );
        assert_eq!(e.info.kind, "npx");
        // An unsafe npx entry falls back to the next one.
        let e = one(
            json!({ "id": "m", "name": "M", "distribution": {
                "binary": { "linux-x86_64": bin }, "npx": { "package": "m@latest" } } }),
            true,
        );
        assert_eq!(e.info.kind, "binary");
        let e = one(json!({ "id": "m", "name": "M", "distribution": {} }), true);
        assert!(e.info.unsupported.is_some());
    }

    #[test]
    fn parses_a_realistic_index() {
        let json = include_str!("../tests/fixtures/acp_registry_sample.json");
        let v = parse_index(json, Some("linux-x86_64"), false).unwrap();
        assert!(v.len() >= 5);
        assert!(v
            .iter()
            .any(|e| e.info.id == "qwen-code" && e.info.kind == "npx"));
        assert!(v.iter().any(|e| e.info.unsupported.is_some()));
    }
}
