//! Optional OS / container sandboxing for agent processes.
//!
//! Supports:
//! - **Bubblewrap (`bwrap`)** on Linux: lightweight user-namespace sandbox that mounts
//!   the host OS read-only, exposes only the agent's worktree (`cwd`) and required
//!   provider auth/config directory read-write, mounts `/tmp` as a private tmpfs, and
//!   unshares the network namespace (`--unshare-net`) unless network access is opted in.
//! - **Docker** cross-platform: runs the agent inside an ephemeral container
//!   (`docker run --rm -i --init`) with only the agent's worktree mounted read-write,
//!   a private `/tmp` tmpfs, `--network none` unless opted in, and only explicitly
//!   sanitized environment variables passed via `-e`.
//!
//! Security invariants:
//! - Never silently downgrade an explicit sandbox requirement: if sandboxing is enabled
//!   for a project (`Auto`, `Bwrap`, or `Docker`) and no matching backend is available,
//!   launching the agent fails cleanly with a clear error instead of running unsandboxed.
//! - Never mount `/` into a container or pass arbitrary host environment variables.
//! - Never allow null bytes, newlines, or flag-shaped values in `docker_image` or
//!   `extra_read_paths`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::bail;
use serde::{Deserialize, Serialize};

use crate::model::{Account, Provider};

/// Default container image used when Docker sandboxing is active without a custom image.
pub const DEFAULT_DOCKER_IMAGE: &str = "node:22-bookworm";

const MAX_DOCKER_IMAGE_LEN: usize = 256;
const MAX_EXTRA_READ_PATHS: usize = 32;
const MAX_EXTRA_PATH_LEN: usize = 4096;

/// Which sandbox backend (if any) wraps agent processes for a project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    #[default]
    Off,
    Auto,
    Bwrap,
    Docker,
}

/// The concrete sandbox backend currently isolating a running agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackend {
    Bwrap,
    Docker,
}

impl SandboxBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            SandboxBackend::Bwrap => "bwrap",
            SandboxBackend::Docker => "docker",
        }
    }
}

/// Active sandbox status attached to a running `Agent` so the UI can display a badge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSandbox {
    pub backend: SandboxBackend,
    pub network: bool,
}

/// Per-project sandbox settings persisted in `Settings.project_sandboxes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectSandboxConfig {
    pub mode: SandboxMode,
    /// Network access is opt-in per project (default `false`).
    pub allow_network: bool,
    /// Container image used in Docker mode (defaults to [`DEFAULT_DOCKER_IMAGE`]).
    pub docker_image: Option<String>,
    /// Optional additional host paths mounted read-only inside the sandbox.
    pub extra_read_paths: Vec<String>,
}

fn trim_opt(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

fn valid_docker_image(img: &str) -> bool {
    if img.is_empty() || img.len() > MAX_DOCKER_IMAGE_LEN || img.starts_with('-') {
        return false;
    }
    img.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b':' | b'@' | b'-'))
}

fn valid_extra_read_path(raw: &str) -> Result<(), &'static str> {
    if raw.len() > MAX_EXTRA_PATH_LEN {
        return Err("An extra read-only path is too long");
    }
    if raw.contains('\0') || raw.contains('\n') || raw.contains('\r') {
        return Err("Extra read-only paths cannot contain control characters");
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.starts_with('-') {
        return Err("Extra read-only paths must be absolute paths");
    }
    let expanded = crate::providers::expand_home(trimmed);
    if !expanded.is_absolute() {
        return Err("Extra read-only paths must be absolute paths (or start with ~)");
    }
    if is_root_path(&expanded) {
        return Err("Mounting the filesystem root (/) as an extra path is not allowed");
    }
    Ok(())
}

fn is_root_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let trimmed = s.trim();
    if trimmed == "/" || trimmed == "\\" {
        return true;
    }
    // Windows drive roots like `C:\` or `C:/`.
    let bytes = trimmed.as_bytes();
    bytes.len() <= 3
        && bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/' || bytes[2] == b'\\')
}

impl ProjectSandboxConfig {
    /// True when this config is equivalent to the default (sandbox off and no custom values).
    pub fn is_default(&self) -> bool {
        self.mode == SandboxMode::Off
            && !self.allow_network
            && trim_opt(&self.docker_image).is_none()
            && self.extra_read_paths.iter().all(|p| p.trim().is_empty())
    }

    /// Docker image to use: trimmed custom image, or [`DEFAULT_DOCKER_IMAGE`].
    pub fn effective_docker_image(&self) -> &str {
        trim_opt(&self.docker_image).unwrap_or(DEFAULT_DOCKER_IMAGE)
    }

    /// Returns a user-facing validation error if any field is invalid.
    pub fn problem(&self) -> Option<String> {
        if let Some(raw) = &self.docker_image {
            if raw.contains('\0') || raw.contains('\n') || raw.contains('\r') {
                return Some("Docker image cannot contain control characters".into());
            }
            let trimmed = raw.trim();
            if !trimmed.is_empty() && !valid_docker_image(trimmed) {
                return Some(format!(
                    "Invalid Docker image \u{201c}{trimmed}\u{201d} (use a standard image name like {DEFAULT_DOCKER_IMAGE})"
                ));
            }
        }
        if self.extra_read_paths.len() > MAX_EXTRA_READ_PATHS {
            return Some(format!(
                "At most {MAX_EXTRA_READ_PATHS} extra read-only paths are allowed"
            ));
        }
        for path in &self.extra_read_paths {
            if let Err(msg) = valid_extra_read_path(path) {
                return Some(msg.into());
            }
        }
        None
    }

    /// Repairs invalid or redundant stored values; returns true if anything changed.
    pub fn sanitize(&mut self) -> bool {
        let mut changed = false;
        if let Some(raw) = self.docker_image.clone() {
            let trimmed = raw.trim();
            if trimmed.is_empty()
                || raw.contains('\0')
                || raw.contains('\n')
                || raw.contains('\r')
                || !valid_docker_image(trimmed)
            {
                self.docker_image = None;
                changed = true;
            } else if trimmed != raw {
                self.docker_image = Some(trimmed.to_string());
                changed = true;
            }
        }

        let orig_len = self.extra_read_paths.len();
        let mut seen = HashSet::new();
        let mut clean = Vec::new();
        for raw in self.extra_read_paths.drain(..) {
            let trimmed = raw.trim();
            if trimmed.is_empty() || valid_extra_read_path(&raw).is_err() {
                changed = true;
                continue;
            }
            if seen.insert(trimmed.to_string()) {
                if clean.len() < MAX_EXTRA_READ_PATHS {
                    if trimmed != raw {
                        changed = true;
                    }
                    clean.push(trimmed.to_string());
                } else {
                    changed = true;
                }
            } else {
                changed = true;
            }
        }
        if clean.len() != orig_len {
            changed = true;
        }
        self.extra_read_paths = clean;
        changed
    }
}

/// Detected sandbox runtime availability on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCapabilities {
    pub bwrap_available: bool,
    pub bwrap_path: Option<String>,
    pub docker_available: bool,
    pub docker_path: Option<String>,
    pub platform: String,
}

/// Detects whether Bubblewrap (`bwrap`, Linux only) and Docker (`docker`, cross-platform)
/// are installed on this machine.
pub fn detect_sandbox_backends() -> SandboxCapabilities {
    let bwrap = if cfg!(target_os = "linux") {
        which::which("bwrap").ok().or_else(|| {
            let fallback = PathBuf::from("/usr/bin/bwrap");
            fallback.is_file().then_some(fallback)
        })
    } else {
        None
    };
    let docker = which::which("docker").ok();
    SandboxCapabilities {
        bwrap_available: bwrap.is_some(),
        bwrap_path: bwrap.map(|p| p.to_string_lossy().into_owned()),
        docker_available: docker.is_some(),
        docker_path: docker.map(|p| p.to_string_lossy().into_owned()),
        platform: std::env::consts::OS.to_string(),
    }
}

/// Looks up the sandbox config for `project_path` (matching exact or normalized path keys).
pub fn config_for_project(
    map: &BTreeMap<String, ProjectSandboxConfig>,
    project_path: &str,
) -> Option<ProjectSandboxConfig> {
    crate::worktree_hooks::map_get(map, project_path).cloned()
}

/// Resolved per-launch sandbox configuration and mounts passed to the protocol drivers.
#[derive(Debug, Clone)]
pub struct LaunchSandbox {
    pub config: ProjectSandboxConfig,
    pub writable_dirs: Vec<PathBuf>,
    pub readonly_dirs: Vec<PathBuf>,
}

/// Inputs for building `bwrap` or `docker run` argument vectors.
#[derive(Debug, Clone)]
pub struct WrapOptions<'a> {
    pub cwd: &'a Path,
    pub allow_network: bool,
    pub docker_image: Option<&'a str>,
    pub env: &'a [(String, String)],
    pub env_remove: &'a [String],
    pub writable_dirs: &'a [PathBuf],
    pub readonly_dirs: &'a [PathBuf],
}

/// Result of [`maybe_wrap_command`]: either the original command or a sandbox-wrapped command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub active: Option<ActiveSandbox>,
}

/// Collects the minimal writable and read-only host mounts needed for an agent inside a sandbox.
///
/// Write access is limited strictly to the agent's `cwd` (always bound by the wrapper) plus
/// the provider's own config/auth directory. Host tools and configured `extra_read_paths` are
/// exposed read-only.
pub fn collect_mounts(
    config: &ProjectSandboxConfig,
    provider: Provider,
    account: &Account,
    data_dir: &Path,
    program: &Path,
    cwd: &Path,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut writable = Vec::new();
    let mut readonly = Vec::new();

    // Provider auth/config directory (needed for session state & OAuth tokens).
    if let Some(dir) = account.config_dir.as_deref().filter(|_| !account.is_system) {
        let p = PathBuf::from(dir);
        if !is_root_path(&p) {
            writable.push(p);
        }
    } else if let Some(home) = crate::providers::home_dir() {
        let rels: &[&str] = match provider {
            Provider::Claude => &[".claude", ".claude.json"],
            Provider::Codex | Provider::Local => &[".codex"],
            Provider::Grok => &[".grok"],
            Provider::Gemini => &[".gemini"],
            Provider::Opencode => &[
                ".config/opencode",
                ".local/share/opencode",
                ".local/state/opencode",
                ".cache/opencode",
            ],
            Provider::Cursor => &[".cursor", ".config/cursor"],
            Provider::Custom => &[],
        };
        for rel in rels {
            let candidate = home.join(rel);
            if candidate.exists() {
                writable.push(candidate);
            }
        }
    }

    // Kvistcode-managed tools, private Node runtime, and image attachments (read-only).
    for sub in ["tools", "node", "attachments"] {
        let p = data_dir.join(sub);
        if !is_root_path(&p) {
            readonly.push(p);
        }
    }

    // If the launched binary lives outside standard system roots (e.g. ~/.local/bin, nvm,
    // or a test mock), bind just that file (or its tool prefix) read-only so bwrap can exec it
    // without exposing the whole home directory.
    if program.is_absolute()
        && !is_standard_system_path(program)
        && !program.starts_with(cwd)
        && !readonly.iter().any(|r| program.starts_with(r))
        && !is_root_path(program)
    {
        readonly.push(program.to_path_buf());
    }

    // User-configured extra read-only paths.
    for raw in &config.extra_read_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() || valid_extra_read_path(trimmed).is_err() {
            continue;
        }
        let expanded = crate::providers::expand_home(trimmed);
        if !is_root_path(&expanded) {
            readonly.push(expanded);
        }
    }

    (dedup_paths(writable, cwd), dedup_paths(readonly, cwd))
}

fn is_standard_system_path(path: &Path) -> bool {
    ["/usr", "/bin", "/lib", "/lib64", "/etc", "/sbin"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn dedup_paths(paths: Vec<PathBuf>, cwd: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    seen.insert(cwd.to_path_buf());
    let mut out = Vec::new();
    for p in paths {
        if !p.as_os_str().is_empty() && !is_root_path(&p) && seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}

/// Builds the argument vector passed to `bwrap` (Bubblewrap on Linux).
///
/// Pure function for straightforward unit testing.
pub fn bwrap_wrap_args(program: &Path, args: &[String], opts: &WrapOptions<'_>) -> Vec<String> {
    let cwd_s = opts.cwd.to_string_lossy().into_owned();
    let mut out: Vec<String> = vec![
        "--ro-bind".into(),
        "/usr".into(),
        "/usr".into(),
        "--ro-bind".into(),
        "/bin".into(),
        "/bin".into(),
        "--ro-bind".into(),
        "/lib".into(),
        "/lib".into(),
        "--ro-bind-try".into(),
        "/lib64".into(),
        "/lib64".into(),
        "--ro-bind-try".into(),
        "/sbin".into(),
        "/sbin".into(),
        "--ro-bind".into(),
        "/etc".into(),
        "/etc".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        // Only the agent's worktree/workspace directory is bound read-write.
        "--bind".into(),
        cwd_s.clone(),
        cwd_s.clone(),
    ];

    let mut mounted: HashSet<PathBuf> = HashSet::new();
    mounted.insert(opts.cwd.to_path_buf());

    for dir in opts.writable_dirs {
        if !dir.as_os_str().is_empty() && !is_root_path(dir) && mounted.insert(dir.clone()) {
            let s = dir.to_string_lossy().into_owned();
            out.push("--bind-try".into());
            out.push(s.clone());
            out.push(s);
        }
    }

    for dir in opts.readonly_dirs {
        if !dir.as_os_str().is_empty() && !is_root_path(dir) && mounted.insert(dir.clone()) {
            let s = dir.to_string_lossy().into_owned();
            out.push("--ro-bind-try".into());
            out.push(s.clone());
            out.push(s);
        }
    }

    if !opts.allow_network {
        out.push("--unshare-net".into());
    }

    out.push("--chdir".into());
    out.push(cwd_s);
    out.push("--die-with-parent".into());
    out.push("--".into());
    out.push(program.to_string_lossy().into_owned());
    out.extend(args.iter().cloned());
    out
}

/// Builds the argument vector passed to `docker` (`docker run ...`).
///
/// Pure function for straightforward unit testing. Never mounts `/` and only forwards
/// explicitly provided, sanitized environment variables (skipping removed keys and
/// unresolved `keychain:` placeholders).
pub fn docker_wrap_args(program: &Path, args: &[String], opts: &WrapOptions<'_>) -> Vec<String> {
    let cwd_s = opts.cwd.to_string_lossy().into_owned();
    let image = opts
        .docker_image
        .map(str::trim)
        .filter(|s| !s.is_empty() && valid_docker_image(s))
        .unwrap_or(DEFAULT_DOCKER_IMAGE);

    let mut out: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        "--init".into(),
        "--workdir".into(),
        cwd_s.clone(),
        "-v".into(),
        format!("{cwd_s}:{cwd_s}:rw"),
        "--tmpfs".into(),
        "/tmp:rw,nosuid,nodev".into(),
    ];

    if !opts.allow_network {
        out.push("--network".into());
        out.push("none".into());
    }

    let mut mounted: HashSet<PathBuf> = HashSet::new();
    mounted.insert(opts.cwd.to_path_buf());

    for dir in opts.writable_dirs {
        if !dir.as_os_str().is_empty() && !is_root_path(dir) && mounted.insert(dir.clone()) {
            let s = dir.to_string_lossy();
            out.push("-v".into());
            out.push(format!("{s}:{s}:rw"));
        }
    }

    for dir in opts.readonly_dirs {
        if !dir.as_os_str().is_empty() && !is_root_path(dir) && mounted.insert(dir.clone()) {
            let s = dir.to_string_lossy();
            out.push("-v".into());
            out.push(format!("{s}:{s}:ro"));
        }
    }

    // Pass only explicitly supplied, non-keychain-placeholder env vars; never leak host env.
    let removed: HashSet<&str> = opts.env_remove.iter().map(String::as_str).collect();
    for (k, v) in opts.env {
        if k.is_empty()
            || k.contains('=')
            || k.contains('\0')
            || v.contains('\0')
            || v.starts_with("keychain:")
            || removed.contains(k.as_str())
        {
            continue;
        }
        out.push("-e".into());
        out.push(format!("{k}={v}"));
    }

    out.push(image.to_string());
    out.push(program.to_string_lossy().into_owned());
    out.extend(args.iter().cloned());
    out
}

/// Wraps `(program, args)` in Bubblewrap or Docker when `sandbox` is enabled, or returns the
/// original command unchanged when sandboxing is off.
///
/// Fails cleanly if a sandbox is required (`Auto`, `Bwrap`, or `Docker`) and no compatible
/// runtime is installed—never silently downgrading to unsandboxed execution.
pub fn maybe_wrap_command(
    sandbox: Option<&LaunchSandbox>,
    program: &Path,
    args: &[String],
    cwd: &Path,
    env: &[(String, String)],
    env_remove: &[String],
) -> anyhow::Result<WrappedCommand> {
    let Some(sb) = sandbox else {
        return Ok(WrappedCommand {
            program: program.to_path_buf(),
            args: args.to_vec(),
            active: None,
        });
    };
    if sb.config.mode == SandboxMode::Off {
        return Ok(WrappedCommand {
            program: program.to_path_buf(),
            args: args.to_vec(),
            active: None,
        });
    }
    let caps = detect_sandbox_backends();
    let wrap_opts = WrapOptions {
        cwd,
        allow_network: sb.config.allow_network,
        docker_image: sb.config.docker_image.as_deref(),
        env,
        env_remove,
        writable_dirs: &sb.writable_dirs,
        readonly_dirs: &sb.readonly_dirs,
    };
    maybe_wrap_command_with_caps(&sb.config, program, args, &wrap_opts, &caps)
}

/// Deterministic core of [`maybe_wrap_command`] with injectable [`SandboxCapabilities`].
pub fn maybe_wrap_command_with_caps(
    config: &ProjectSandboxConfig,
    program: &Path,
    args: &[String],
    opts: &WrapOptions<'_>,
    caps: &SandboxCapabilities,
) -> anyhow::Result<WrappedCommand> {
    if config.mode == SandboxMode::Off {
        return Ok(WrappedCommand {
            program: program.to_path_buf(),
            args: args.to_vec(),
            active: None,
        });
    }

    if let Some(problem) = config.problem() {
        bail!("Invalid sandbox configuration: {problem}");
    }

    let (backend, backend_path) = match config.mode {
        SandboxMode::Off => unreachable!(),
        SandboxMode::Auto => {
            if let Some(p) = caps.bwrap_path.as_deref().filter(|_| caps.bwrap_available) {
                (SandboxBackend::Bwrap, p)
            } else if let Some(p) = caps
                .docker_path
                .as_deref()
                .filter(|_| caps.docker_available)
            {
                (SandboxBackend::Docker, p)
            } else {
                bail!(
                    "Sandbox is enabled for this project (Auto), but neither Bubblewrap (bwrap) nor Docker is installed. Install bwrap or Docker, or turn off sandboxing in Project settings."
                );
            }
        }
        SandboxMode::Bwrap => {
            if let Some(p) = caps.bwrap_path.as_deref().filter(|_| caps.bwrap_available) {
                (SandboxBackend::Bwrap, p)
            } else if caps.platform != "linux" {
                bail!(
                    "Bubblewrap (bwrap) sandboxing is only available on Linux. Switch this project's sandbox mode to Docker or Off in Project settings."
                );
            } else {
                bail!(
                    "Bubblewrap (bwrap) is required by this project's sandbox setting, but bwrap was not found on PATH. Install bubblewrap or change the sandbox mode in Project settings."
                );
            }
        }
        SandboxMode::Docker => {
            if let Some(p) = caps
                .docker_path
                .as_deref()
                .filter(|_| caps.docker_available)
            {
                (SandboxBackend::Docker, p)
            } else {
                bail!(
                    "Docker is required by this project's sandbox setting, but docker was not found on PATH. Install Docker or change the sandbox mode in Project settings."
                );
            }
        }
    };

    let wrapped_args = match backend {
        SandboxBackend::Bwrap => bwrap_wrap_args(program, args, opts),
        SandboxBackend::Docker => docker_wrap_args(program, args, opts),
    };

    Ok(WrappedCommand {
        program: PathBuf::from(backend_path),
        args: wrapped_args,
        active: Some(ActiveSandbox {
            backend,
            network: opts.allow_network,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linux_caps(bwrap: bool, docker: bool) -> SandboxCapabilities {
        SandboxCapabilities {
            bwrap_available: bwrap,
            bwrap_path: bwrap.then(|| "/usr/bin/bwrap".into()),
            docker_available: docker,
            docker_path: docker.then(|| "/usr/bin/docker".into()),
            platform: "linux".into(),
        }
    }

    fn mac_caps(docker: bool) -> SandboxCapabilities {
        SandboxCapabilities {
            bwrap_available: false,
            bwrap_path: None,
            docker_available: docker,
            docker_path: docker.then(|| "/usr/local/bin/docker".into()),
            platform: "macos".into(),
        }
    }

    fn sample_opts<'a>(
        cwd: &'a Path,
        allow_network: bool,
        docker_image: Option<&'a str>,
        env: &'a [(String, String)],
        env_remove: &'a [String],
        writable_dirs: &'a [PathBuf],
        readonly_dirs: &'a [PathBuf],
    ) -> WrapOptions<'a> {
        WrapOptions {
            cwd,
            allow_network,
            docker_image,
            env,
            env_remove,
            writable_dirs,
            readonly_dirs,
        }
    }

    #[test]
    fn bwrap_wrap_args_isolates_fs_and_network_by_default() {
        let cwd = Path::new("/home/alice/.local/share/dev.armada.app/worktrees/proj/a1");
        let writable = vec![PathBuf::from("/home/alice/.claude")];
        let readonly = vec![
            PathBuf::from("/home/alice/.local/share/dev.armada.app/tools"),
            PathBuf::from("/opt/shared-sdk"),
        ];
        let opts = sample_opts(cwd, false, None, &[], &[], &writable, &readonly);
        let args = bwrap_wrap_args(
            Path::new("/usr/bin/node"),
            &["agent.js".into(), "--stdio".into()],
            &opts,
        );

        assert!(args.windows(3).any(|w| w == ["--ro-bind", "/usr", "/usr"]));
        assert!(args.windows(3).any(|w| w == ["--ro-bind", "/bin", "/bin"]));
        assert!(args.windows(3).any(|w| w == ["--ro-bind", "/lib", "/lib"]));
        assert!(args
            .windows(3)
            .any(|w| w == ["--ro-bind-try", "/lib64", "/lib64"]));
        assert!(args.windows(3).any(|w| w == ["--ro-bind", "/etc", "/etc"]));
        assert!(args.windows(2).any(|w| w == ["--proc", "/proc"]));
        assert!(args.windows(2).any(|w| w == ["--dev", "/dev"]));
        assert!(args.windows(2).any(|w| w == ["--tmpfs", "/tmp"]));
        let cwd_s = cwd.to_string_lossy();
        assert!(args.windows(3).any(|w| w == ["--bind", &cwd_s, &cwd_s]));
        assert!(args
            .windows(3)
            .any(|w| w == ["--bind-try", "/home/alice/.claude", "/home/alice/.claude"]));
        assert!(args.windows(3).any(|w| {
            w == [
                "--ro-bind-try",
                "/home/alice/.local/share/dev.armada.app/tools",
                "/home/alice/.local/share/dev.armada.app/tools",
            ]
        }));
        assert!(args.contains(&"--unshare-net".to_string()));
        assert!(args.windows(2).any(|w| w == ["--chdir", &cwd_s]));
        assert!(args.contains(&"--die-with-parent".to_string()));

        // Ends with `-- <program> <args...>`.
        let sep = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(&args[sep + 1..], ["/usr/bin/node", "agent.js", "--stdio"]);
    }

    #[test]
    fn bwrap_wrap_args_omits_unshare_net_when_network_allowed() {
        let cwd = Path::new("/tmp/wt");
        let opts = sample_opts(cwd, true, None, &[], &[], &[], &[]);
        let args = bwrap_wrap_args(Path::new("claude"), &[], &opts);
        assert!(!args.contains(&"--unshare-net".to_string()));
    }

    #[test]
    fn bwrap_never_binds_root_from_extra_dirs() {
        let cwd = Path::new("/tmp/wt");
        let writable = vec![PathBuf::from("/")];
        let readonly = vec![PathBuf::from("/")];
        let opts = sample_opts(cwd, false, None, &[], &[], &writable, &readonly);
        let args = bwrap_wrap_args(Path::new("node"), &[], &opts);
        assert!(!args.windows(3).any(|w| w == ["--bind-try", "/", "/"]));
        assert!(!args.windows(3).any(|w| w == ["--ro-bind-try", "/", "/"]));
    }

    #[test]
    fn docker_wrap_args_isolates_fs_network_and_env() {
        let cwd = Path::new("/workspace/wt-1");
        let writable = vec![PathBuf::from("/home/alice/.codex")];
        let readonly = vec![PathBuf::from("/opt/tools"), PathBuf::from("/")];
        let env = vec![
            ("OPENAI_API_KEY".into(), "sk-test-123".into()),
            ("BAD_SECRET".into(), "keychain:acct-1".into()),
            ("REMOVED_VAR".into(), "should-not-appear".into()),
        ];
        let env_remove = vec!["REMOVED_VAR".into()];
        let opts = sample_opts(
            cwd,
            false,
            Some("node:22-slim"),
            &env,
            &env_remove,
            &writable,
            &readonly,
        );
        let args = docker_wrap_args(
            Path::new("codex"),
            &["app-server".into(), "--stdio".into()],
            &opts,
        );

        assert_eq!(&args[0..4], ["run", "--rm", "-i", "--init"]);
        assert!(args
            .windows(2)
            .any(|w| w == ["--workdir", "/workspace/wt-1"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["-v", "/workspace/wt-1:/workspace/wt-1:rw"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--tmpfs", "/tmp:rw,nosuid,nodev"]));
        assert!(args.windows(2).any(|w| w == ["--network", "none"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["-v", "/home/alice/.codex:/home/alice/.codex:rw"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["-v", "/opt/tools:/opt/tools:ro"]));
        // Never mounts root.
        assert!(!args.iter().any(|a| a.starts_with("/:/:")));
        // Only sanitized env passed; keychain placeholder and removed vars dropped.
        assert!(args
            .windows(2)
            .any(|w| w == ["-e", "OPENAI_API_KEY=sk-test-123"]));
        assert!(!args.iter().any(|a| a.contains("BAD_SECRET")));
        assert!(!args.iter().any(|a| a.contains("REMOVED_VAR")));
        // Ends with `<image> <program> <args...>`.
        let tail = &args[args.len() - 4..];
        assert_eq!(tail, ["node:22-slim", "codex", "app-server", "--stdio"]);
    }

    #[test]
    fn docker_wrap_args_uses_default_image_and_allows_network_when_opted_in() {
        let cwd = Path::new("/workspace/wt-1");
        let opts = sample_opts(cwd, true, None, &[], &[], &[], &[]);
        let args = docker_wrap_args(Path::new("claude"), &["--acp".into()], &opts);
        assert!(!args.contains(&"--network".to_string()));
        assert!(args.contains(&DEFAULT_DOCKER_IMAGE.to_string()));
    }

    #[test]
    fn maybe_wrap_off_passes_through_unchanged() {
        let cfg = ProjectSandboxConfig::default();
        let cwd = Path::new("/tmp/wt");
        let opts = sample_opts(cwd, false, None, &[], &[], &[], &[]);
        let res = maybe_wrap_command_with_caps(
            &cfg,
            Path::new("/usr/bin/claude"),
            &["--acp".into()],
            &opts,
            &linux_caps(false, false),
        )
        .unwrap();
        assert_eq!(res.program, PathBuf::from("/usr/bin/claude"));
        assert_eq!(res.args, vec!["--acp"]);
        assert_eq!(res.active, None);
    }

    #[test]
    fn maybe_wrap_auto_prefers_bwrap_then_docker_and_never_silently_downgrades() {
        let cfg = ProjectSandboxConfig {
            mode: SandboxMode::Auto,
            allow_network: false,
            docker_image: None,
            extra_read_paths: vec![],
        };
        let cwd = Path::new("/tmp/wt");
        let opts = sample_opts(cwd, false, None, &[], &[], &[], &[]);

        // Both available -> prefers bwrap.
        let both = maybe_wrap_command_with_caps(
            &cfg,
            Path::new("claude"),
            &[],
            &opts,
            &linux_caps(true, true),
        )
        .unwrap();
        assert_eq!(both.program, PathBuf::from("/usr/bin/bwrap"));
        assert_eq!(
            both.active,
            Some(ActiveSandbox {
                backend: SandboxBackend::Bwrap,
                network: false
            })
        );

        // Only docker -> uses docker.
        let docker_only =
            maybe_wrap_command_with_caps(&cfg, Path::new("claude"), &[], &opts, &mac_caps(true))
                .unwrap();
        assert_eq!(docker_only.program, PathBuf::from("/usr/local/bin/docker"));
        assert_eq!(
            docker_only.active,
            Some(ActiveSandbox {
                backend: SandboxBackend::Docker,
                network: false
            })
        );

        // Neither available -> fails cleanly (NEVER silently runs unsandboxed).
        let err = maybe_wrap_command_with_caps(
            &cfg,
            Path::new("claude"),
            &[],
            &opts,
            &linux_caps(false, false),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("Bubblewrap") && err.contains("Docker"),
            "{err}"
        );
    }

    #[test]
    fn explicit_bwrap_or_docker_fails_cleanly_when_missing() {
        let cwd = Path::new("/tmp/wt");
        let opts = sample_opts(cwd, true, None, &[], &[], &[], &[]);

        let bwrap_cfg = ProjectSandboxConfig {
            mode: SandboxMode::Bwrap,
            allow_network: true,
            ..Default::default()
        };
        // Missing on Linux.
        let err = maybe_wrap_command_with_caps(
            &bwrap_cfg,
            Path::new("claude"),
            &[],
            &opts,
            &linux_caps(false, true),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("bwrap"), "{err}");

        // Requested on macOS -> clear Linux-only error.
        let err = maybe_wrap_command_with_caps(
            &bwrap_cfg,
            Path::new("claude"),
            &[],
            &opts,
            &mac_caps(true),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("only available on Linux"), "{err}");

        // Explicit Docker missing even when bwrap exists.
        let docker_cfg = ProjectSandboxConfig {
            mode: SandboxMode::Docker,
            allow_network: true,
            ..Default::default()
        };
        let err = maybe_wrap_command_with_caps(
            &docker_cfg,
            Path::new("claude"),
            &[],
            &opts,
            &linux_caps(true, false),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Docker"), "{err}");
    }

    // The paths are Linux mount paths (Bubblewrap/Docker); `/opt/sdk` is not absolute on Windows.
    #[cfg(unix)]
    #[test]
    fn config_validation_and_sanitization() {
        let mut cfg = ProjectSandboxConfig {
            mode: SandboxMode::Docker,
            allow_network: true,
            docker_image: Some("  node:22-bookworm  ".into()),
            extra_read_paths: vec!["/opt/sdk".into(), "/opt/sdk".into(), " ".into()],
        };
        assert!(cfg.problem().is_none());
        assert!(cfg.sanitize());
        assert_eq!(cfg.docker_image.as_deref(), Some("node:22-bookworm"));
        assert_eq!(cfg.extra_read_paths, vec!["/opt/sdk"]);
        assert!(!cfg.sanitize());

        // Rejects flag-like or null-byte docker image.
        let bad_img = ProjectSandboxConfig {
            docker_image: Some("--privileged".into()),
            ..Default::default()
        };
        assert!(bad_img.problem().is_some());
        let null_img = ProjectSandboxConfig {
            docker_image: Some("node\0:22".into()),
            ..Default::default()
        };
        assert!(null_img.problem().is_some());

        // Rejects null bytes, relative paths, root mount, and over-limit extra paths.
        let bad_path = ProjectSandboxConfig {
            extra_read_paths: vec!["relative/dir".into()],
            ..Default::default()
        };
        assert!(bad_path.problem().is_some());
        let null_path = ProjectSandboxConfig {
            extra_read_paths: vec!["/opt/\0bad".into()],
            ..Default::default()
        };
        assert!(null_path.problem().is_some());
        let root_path = ProjectSandboxConfig {
            extra_read_paths: vec!["/".into()],
            ..Default::default()
        };
        assert!(root_path.problem().is_some());
        let too_many = ProjectSandboxConfig {
            extra_read_paths: (0..=MAX_EXTRA_READ_PATHS)
                .map(|i| format!("/opt/p{i}"))
                .collect(),
            ..Default::default()
        };
        assert!(too_many.problem().is_some());

        // Sanitize cleans all bad entries.
        let mut dirty = ProjectSandboxConfig {
            mode: SandboxMode::Auto,
            allow_network: false,
            docker_image: Some("--bad".into()),
            extra_read_paths: vec![
                "/".into(),
                "rel".into(),
                "/opt/a\0b".into(),
                "/opt/good".into(),
            ],
        };
        assert!(dirty.sanitize());
        assert_eq!(dirty.docker_image, None);
        assert_eq!(dirty.extra_read_paths, vec!["/opt/good"]);
        assert!(dirty.problem().is_none());
    }

    #[test]
    fn serde_round_trip_and_defaults() {
        let empty: ProjectSandboxConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, ProjectSandboxConfig::default());
        assert_eq!(empty.mode, SandboxMode::Off);
        assert!(!empty.allow_network);
        assert!(empty.is_default());

        let active = ActiveSandbox {
            backend: SandboxBackend::Bwrap,
            network: false,
        };
        let v = serde_json::to_value(&active).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "backend": "bwrap", "network": false })
        );
        let back: ActiveSandbox = serde_json::from_value(v).unwrap();
        assert_eq!(back, active);
    }
}
