//! Google Antigravity CLI paths and Google sign-in.
//!
//! Antigravity accepts explicit app-data and Gemini-data directories. Google sign-in runs
//! through ACP's `authenticate`, which opens a browser and saves the choice.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;

use crate::model::{now_ms, Account, AccountUsage, AuthKind, Provider, UsageWindow};
use crate::providers;

/// The Node script behind the sign-in: starts `agy acp`, asks it to sign in with
/// Google, prints how it went and exits (0 when signed in).
const LOGIN_SCRIPT: &str = r#"// Kvistcode: signs Gemini CLI in with Google, then exits.
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const cli = process.argv[2];
const win = process.platform === "win32";
const profile = process.env.KVISTCODE_GEMINI_PROFILE || path.join(os.homedir(), ".gemini");
// Keep the server in its own process group so closing the sign-in window stops it.
const appData = path.join(profile, "antigravity-cli");
const child = spawn(cli, ["acp", "--app_data_dir", "antigravity-cli", "--gemini-dir", profile], {
  stdio: ["pipe", "pipe", "inherit"],
  shell: false,
  detached: !win,
});
const stop = () => {
  try { if (!win) process.kill(-child.pid); else child.kill(); } catch {}
};
let buf = "";
let done = false;
const send = (id, method, params) =>
  child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
const finish = (code, text) => {
  if (done) return;
  done = true;
  console.log(text);
  stop();
  process.exit(code);
};
for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) process.on(sig, () => { stop(); process.exit(1); });
child.on("error", (e) => finish(1, "Couldn't start Antigravity CLI: " + e.message));
child.on("exit", () => finish(1, "Antigravity CLI stopped before the sign-in finished."));
child.stdout.on("data", (chunk) => {
  buf += chunk;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    let msg;
    try { msg = JSON.parse(line); } catch { continue; }
    if (msg.id === 1) {
      if (msg.error) return finish(1, "Antigravity CLI didn't start: " + msg.error.message);
      console.log("Signing in to Gemini with your Google account.");
      console.log("A browser window opens; finish signing in there.\n");
      send(2, "authenticate", { methodId: "oauth-personal" });
    } else if (msg.id === 2) {
      if (msg.error) return finish(1, "Sign-in didn't finish: " + msg.error.message);
      fs.mkdirSync(appData, { recursive: true });
      fs.writeFileSync(path.join(appData, "kvistcode-authenticated"), "");
      finish(0, "Signed in. You can close this window.");
    }
  }
});
send(1, "initialize", { protocolVersion: 1, clientCapabilities: {} });
"#;

/// The account's Gemini folder (`.gemini` under its own home, or the user's for the
/// system account).
pub fn config_dir(account: &Account) -> Option<PathBuf> {
    account
        .config_dir
        .clone()
        .filter(|_| !account.is_system)
        .map(PathBuf::from)
        .or_else(providers::home_dir)
        .map(|h| h.join(".gemini"))
}

/// ACP arguments for a profile. Antigravity expects `--app_data_dir` to be a relative
/// directory name under its Gemini data directory, not the absolute path it resolves to.
pub(crate) fn acp_args(profile: &Path) -> Vec<String> {
    vec![
        "acp".into(),
        "--app_data_dir".into(),
        "antigravity-cli".into(),
        "--gemini-dir".into(),
        profile.to_string_lossy().into_owned(),
    ]
}

/// Project instruction files a Kvistcode-made Gemini profile reads: AGENTS.md (what Codex,
/// Grok and Kvistcode's own tools use) as well as Gemini's GEMINI.md.
const CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "GEMINI.md"];

/// Points a dedicated account's Gemini profile at [`CONTEXT_FILES`], unless it already
/// names its own. Kvistcode made that folder; the system account's `~/.gemini` is the
/// person's own and is never written. Best effort: Gemini runs either way.
pub fn prepare_home(account: &Account) {
    if account.is_system || account.config_dir.is_none() {
        return;
    }
    let Some(dir) = config_dir(account) else {
        return;
    };
    let path = dir.join("antigravity-cli").join("settings.json");
    let mut settings: serde_json::Value = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            // Comments or a typo: leave the person's file alone.
            Err(_) => return,
        },
        Err(_) => serde_json::json!({}),
    };
    let Some(root) = settings.as_object_mut() else {
        return;
    };
    let context = root
        .entry("context")
        .or_insert_with(|| serde_json::json!({}));
    let Some(context) = context.as_object_mut() else {
        return;
    };
    if context.contains_key("fileName") {
        if account.auth_kind == crate::model::AuthKind::ApiKey {
            root.insert("modelProvider".into(), serde_json::json!("gemini"));
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(text) = serde_json::to_string_pretty(&settings) {
                let _ = std::fs::write(&path, text);
            }
        }
        return;
    }
    context.insert("fileName".into(), serde_json::json!(CONTEXT_FILES));
    if account.auth_kind == crate::model::AuthKind::ApiKey {
        root.insert("modelProvider".into(), serde_json::json!("gemini"));
    }
    if std::fs::create_dir_all(path.parent().unwrap_or(&dir)).is_ok() {
        if let Ok(text) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(&path, text);
        }
    }
}

/// `node <script> <gemini>`: the Google sign-in for the account the environment points at
/// (see `accounts::account_env`). Writes the script into Kvistcode's tools folder first.
pub fn login_command(data_dir: &Path) -> Option<(String, Vec<String>)> {
    let cli = providers::find_cli(Provider::Gemini)?;
    // Kvistcode's own Node is first on PATH when it has one (see `toolchain`).
    let node = providers::find_node().unwrap_or_else(|| PathBuf::from("node"));
    let dir = crate::toolchain::tools_dir(data_dir);
    let script = dir.join("gemini-sign-in.js");
    if std::fs::read_to_string(&script).ok().as_deref() != Some(LOGIN_SCRIPT) {
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&script, LOGIN_SCRIPT).ok()?;
    }
    Some((
        node.to_string_lossy().into_owned(),
        vec![
            script.to_string_lossy().into_owned(),
            cli.to_string_lossy().into_owned(),
        ],
    ))
}

/// The successful ACP authentication marker is non-sensitive. Antigravity keeps the
/// actual OAuth token in the OS keyring, so Kvistcode never reads or copies it.
pub fn google_sign_in(dir: &Path) -> (bool, Option<String>) {
    (
        dir.join("antigravity-cli")
            .join("kvistcode-authenticated")
            .is_file(),
        None,
    )
}

const USAGE_TIMEOUT: Duration = Duration::from_secs(20);

/// Fetches Antigravity's subscription usage windows via `agy --print /usage --output-format json`.
/// Returns `None` for API-key accounts, when the CLI isn't installed, or when the CLI doesn't
/// report structured quota (best effort, never blocks sign-in or agents).
pub async fn fetch_usage(account: &Account) -> Option<AccountUsage> {
    if account.provider != Provider::Gemini || account.auth_kind == AuthKind::ApiKey {
        return None;
    }
    let (program, base_args) = if let Ok(cmd) = std::env::var("KVISTCODE_GEMINI_USAGE_COMMAND") {
        let mut parts = cmd.split_whitespace().map(str::to_string);
        let prog = parts.next()?;
        (
            which::which(&prog).unwrap_or_else(|_| PathBuf::from(prog)),
            parts.collect::<Vec<_>>(),
        )
    } else {
        (providers::find_cli(Provider::Gemini)?, Vec::new())
    };
    let env = crate::accounts::account_env(account);
    let env_remove = crate::accounts::env_to_remove(account);
    let args = if base_args.is_empty() {
        let mut args = vec![
            "--print".into(),
            "/usage".into(),
            "--output-format".into(),
            "json".into(),
        ];
        // Dedicated accounts stay strictly isolated to their own profile directory;
        // never fall back to the default system profile, which would leak another account's quota.
        if let Some(dir) = config_dir(account).filter(|_| !account.is_system) {
            args.extend([
                "--app_data_dir".into(),
                "antigravity-cli".into(),
                "--gemini-dir".into(),
                dir.to_string_lossy().into_owned(),
            ]);
        }
        args
    } else {
        base_args
    };
    let mut cmd = crate::accounts::quiet_command(&program);
    for var in &env_remove {
        cmd.env_remove(var);
    }
    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = match tokio::time::timeout(USAGE_TIMEOUT, cmd.output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            tracing::debug!(account = %account.id, "gemini usage command failed to start: {e}");
            return None;
        }
        Err(_) => {
            tracing::debug!(account = %account.id, "gemini usage command timed out");
            return None;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    if out.status.success() {
        if let Some(usage) = parse_usage_output(&stdout) {
            return Some(usage);
        }
        tracing::debug!(
            account = %account.id,
            stdout_len = stdout.len(),
            "gemini usage command succeeded without parseable quota"
        );
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = crate::redact::redact_secrets(stderr.trim());
        tracing::debug!(
            account = %account.id,
            code = ?out.status.code(),
            stderr = %detail.chars().take(200).collect::<String>(),
            "gemini usage command exited non-zero"
        );
    }
    None
}

/// Parses stdout from `agy --print /usage --output-format json`, tolerating log lines
/// (even ones containing `{`), NDJSON streams, or plain-text `/usage` output.
pub fn parse_usage_output(raw: &str) -> Option<AccountUsage> {
    let mut search_from = 0;
    while let Some(rel) = raw[search_from..].find('{') {
        let start = search_from + rel;
        let mut de = serde_json::Deserializer::from_str(&raw[start..]).into_iter::<Value>();
        match de.next() {
            Some(Ok(v)) => {
                if let Some(usage) = parse_quota_value(&v) {
                    return Some(usage);
                }
                let advance = de.byte_offset().max(1);
                search_from = start + advance;
            }
            _ => {
                search_from = start + 1;
            }
        }
    }
    parse_usage_text(raw)
}

/// Converts an Antigravity / Google quota payload (`command.data.groups[].buckets[]`,
/// `response.groups[].buckets[]`, `data.groups[].buckets[]`, `groups[].buckets[]`,
/// `buckets[]`, or a text summary inside `response`/`result`) into an [`AccountUsage`] snapshot.
pub(crate) fn parse_quota_value(v: &Value) -> Option<AccountUsage> {
    if !v.is_object() {
        return None;
    }
    let outer_plan = extract_plan(v);
    let mut candidates: Vec<&Value> = Vec::with_capacity(10);
    if v["command"]["data"].is_object() {
        candidates.push(&v["command"]["data"]);
    }
    if v["command"].is_object() {
        candidates.push(&v["command"]);
    }
    for key in [
        "data",
        "response",
        "result",
        "structured_output",
        "structuredOutput",
        "quotaSummary",
        "quota_summary",
        "userQuotaSummary",
        "user_quota_summary",
        "quota",
        "rateLimits",
        "rate_limits",
    ] {
        if v[key].is_object() {
            candidates.push(&v[key]);
            if v[key]["data"].is_object() {
                candidates.push(&v[key]["data"]);
            }
        }
    }
    candidates.push(v);

    for root in candidates {
        if let Some(buckets) = select_quota_buckets(root) {
            let mut parsed: Vec<UsageWindow> =
                buckets.iter().filter_map(parse_quota_bucket).collect();
            if !parsed.is_empty() {
                // Shortest window (e.g. 5h) first -> primary; longer window (e.g. weekly/daily) -> secondary.
                parsed.sort_by_key(|w| w.window_mins.unwrap_or(i64::MAX));
                let mut iter = parsed.into_iter();
                let primary = iter.next();
                let secondary = iter.next();
                let plan = extract_plan(root).or_else(|| outer_plan.clone());
                return Some(AccountUsage {
                    primary,
                    secondary,
                    fable: None,
                    plan,
                    updated_at: now_ms(),
                });
            }
        }
    }

    // Some CLI versions wrap the `/usage` text summary inside a string `response`/`result`/`output`.
    for key in ["response", "result", "output", "text"] {
        if let Some(s) = v[key].as_str() {
            if let Some(mut usage) = parse_usage_text(s) {
                if usage.plan.is_none() {
                    usage.plan = outer_plan.clone();
                }
                return Some(usage);
            }
        }
    }
    None
}

fn extract_plan(v: &Value) -> Option<String> {
    let from_field = |f: &Value| -> Option<String> {
        if let Some(s) = f.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
        if f.is_object() {
            for k in ["name", "displayName", "display_name", "title", "tier", "id"] {
                if let Some(s) = f[k].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    return Some(s.to_string());
                }
            }
        }
        None
    };
    for key in [
        "plan",
        "planType",
        "plan_type",
        "tier",
        "subscriptionTier",
        "subscription_tier",
    ] {
        if let Some(p) = from_field(&v[key]) {
            return Some(p);
        }
    }
    None
}

fn buckets_in(obj: &Value) -> Option<&[Value]> {
    for key in [
        "buckets",
        "quotaBuckets",
        "quota_buckets",
        "windows",
        "limits",
        "quotas",
    ] {
        if let Some(arr) = obj[key].as_array().filter(|a| !a.is_empty()) {
            return Some(arr.as_slice());
        }
    }
    None
}

fn select_quota_buckets(root: &Value) -> Option<&[Value]> {
    for groups_key in [
        "groups",
        "modelGroups",
        "model_groups",
        "quotaGroups",
        "quota_groups",
        "pools",
    ] {
        if let Some(groups) = root[groups_key].as_array() {
            // Prefer the Gemini model pool over third-party pools (e.g. 3p-models).
            let is_gemini = |g: &&Value| {
                let id = g["groupId"]
                    .as_str()
                    .or_else(|| g["group_id"].as_str())
                    .or_else(|| g["id"].as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let name = g["displayName"]
                    .as_str()
                    .or_else(|| g["display_name"].as_str())
                    .or_else(|| g["name"].as_str())
                    .or_else(|| g["label"].as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let has_gemini_model = g["models"].as_array().is_some_and(|ms| {
                    ms.iter().any(|m| {
                        m.as_str()
                            .or_else(|| m["id"].as_str())
                            .or_else(|| m["name"].as_str())
                            .is_some_and(|s| s.to_ascii_lowercase().contains("gemini"))
                    })
                });
                id.contains("gemini") || name.contains("gemini") || has_gemini_model
            };
            let chosen = groups
                .iter()
                .filter(|g| buckets_in(g).is_some())
                .find(is_gemini)
                .or_else(|| groups.iter().find(|g| buckets_in(g).is_some()));
            if let Some(g) = chosen {
                return buckets_in(g);
            }
        }
    }
    buckets_in(root)
}

fn parse_quota_bucket(b: &Value) -> Option<UsageWindow> {
    let used_percent = if let Some(rem) = b["remainingFraction"]
        .as_f64()
        .or_else(|| b["remaining_fraction"].as_f64())
    {
        ((1.0 - rem) * 100.0).clamp(0.0, 100.0)
    } else if let Some(rem_pct) = b["remainingPercentage"]
        .as_f64()
        .or_else(|| b["remaining_percentage"].as_f64())
        .or_else(|| b["remainingPercent"].as_f64())
        .or_else(|| b["remaining_percent"].as_f64())
        .or_else(|| b["percentRemaining"].as_f64())
        .or_else(|| b["percent_remaining"].as_f64())
    {
        (100.0 - rem_pct).clamp(0.0, 100.0)
    } else if let Some(u) = b["usedPercent"]
        .as_f64()
        .or_else(|| b["used_percent"].as_f64())
        .or_else(|| b["percentUsed"].as_f64())
        .or_else(|| b["percent_used"].as_f64())
    {
        u.clamp(0.0, 100.0)
    } else if let Some(uf) = b["usedFraction"]
        .as_f64()
        .or_else(|| b["used_fraction"].as_f64())
    {
        (uf * 100.0).clamp(0.0, 100.0)
    } else {
        let u = b["utilization"].as_f64()?;
        let pct = if u <= 1.0 { u * 100.0 } else { u };
        pct.clamp(0.0, 100.0)
    };
    let window_mins = b["windowDurationMins"]
        .as_i64()
        .or_else(|| b["window_duration_mins"].as_i64())
        .or_else(|| b["windowMins"].as_i64())
        .or_else(|| b["window_mins"].as_i64())
        .or_else(|| {
            b["windowDurationSeconds"]
                .as_i64()
                .or_else(|| b["window_duration_seconds"].as_i64())
                .or_else(|| b["windowSeconds"].as_i64())
                .or_else(|| b["window_seconds"].as_i64())
                .filter(|&s| s >= 60)
                .map(|s| s / 60)
        })
        .or_else(|| {
            [
                "window",
                "windowLabel",
                "window_label",
                "bucketId",
                "bucket_id",
                "id",
                "displayName",
                "display_name",
                "name",
                "label",
                "period",
                "type",
            ]
            .iter()
            .filter_map(|k| b[*k].as_str())
            .find_map(parse_window_mins)
        });
    let resets_at = parse_reset_timestamp(
        b.get("resetTime")
            .or_else(|| b.get("reset_time"))
            .or_else(|| b.get("resetsAt"))
            .or_else(|| b.get("resets_at"))
            .or_else(|| b.get("resetAt"))
            .or_else(|| b.get("reset_at")),
    );
    Some(UsageWindow {
        used_percent,
        window_mins,
        resets_at,
    })
}

/// Fallback parser for plain-text `/usage` summaries (e.g. `5-hour window: 85% remaining`).
/// Prefers lines under a Gemini section if the summary lists multiple model pools.
fn parse_usage_text(raw: &str) -> Option<AccountUsage> {
    let mut gemini_windows: Vec<UsageWindow> = Vec::new();
    let mut all_windows: Vec<UsageWindow> = Vec::new();
    let mut in_gemini_section = false;
    let mut saw_other_section = false;
    let now = now_ms();
    let tz = jiff::tz::TimeZone::system();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        let has_pct = trimmed.contains('%');
        if !has_pct {
            if lower.contains("gemini") {
                in_gemini_section = true;
            } else if lower.contains("claude")
                || lower.contains("gpt")
                || lower.contains("3p")
                || lower.contains("third-party")
            {
                in_gemini_section = false;
                saw_other_section = true;
            }
            continue;
        }
        let Some(window_mins) = parse_window_mins(&lower) else {
            continue;
        };
        let Some(pct) = extract_percent(&lower) else {
            continue;
        };
        let is_remaining = lower.contains("remaining") || lower.contains("left");
        let is_used = lower.contains("used") || lower.contains("utilized");
        if !is_remaining && !is_used {
            continue;
        }
        let used_percent = if is_remaining { 100.0 - pct } else { pct }.clamp(0.0, 100.0);
        let resets_at = crate::limit_reset::parse_reset(trimmed, now, &tz);
        let win = UsageWindow {
            used_percent,
            window_mins: Some(window_mins),
            resets_at,
        };
        if in_gemini_section {
            gemini_windows.push(win.clone());
        }
        if !saw_other_section || in_gemini_section {
            all_windows.push(win);
        }
    }

    let mut chosen = if !gemini_windows.is_empty() {
        gemini_windows
    } else {
        all_windows
    };
    if chosen.is_empty() {
        return None;
    }
    chosen.sort_by_key(|w| w.window_mins.unwrap_or(i64::MAX));
    chosen.dedup_by_key(|w| w.window_mins);
    let mut iter = chosen.into_iter();
    Some(AccountUsage {
        primary: iter.next(),
        secondary: iter.next(),
        fable: None,
        plan: None,
        updated_at: now,
    })
}

fn extract_percent(s: &str) -> Option<f64> {
    let idx = s.find('%')?;
    let before = s[..idx].trim_end();
    let start = before
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(0, |i| i + 1);
    let num = before[start..].parse::<f64>().ok()?;
    (0.0..=100.0).contains(&num).then_some(num)
}

pub(crate) fn parse_window_mins(label: &str) -> Option<i64> {
    let l = label.trim().to_ascii_lowercase();
    if l.contains("5h")
        || l.contains("5-h")
        || l.contains("5 h")
        || l.contains("5 hour")
        || l.contains("5 hr")
        || l.contains("five_hour")
        || l.contains("five-hour")
    {
        return Some(300);
    }
    if l.contains("week")
        || l.contains("7d")
        || l.contains("7-d")
        || l.contains("7 day")
        || l.contains("seven_day")
        || l.contains("seven-day")
    {
        return Some(10080);
    }
    if l.contains("daily")
        || l.contains("24h")
        || l.contains("24-h")
        || l.contains("24 hour")
        || l.contains("1d")
        || l.contains("1 day")
        || l.contains("day")
    {
        return Some(1440);
    }
    if let Some(hours) = l.strip_suffix('h').and_then(|n| n.parse::<i64>().ok()) {
        return (hours > 0).then_some(hours.saturating_mul(60));
    }
    if let Some(mins) = l.strip_suffix('m').and_then(|n| n.parse::<i64>().ok()) {
        return (mins > 0).then_some(mins);
    }
    None
}

pub(crate) fn parse_reset_timestamp(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if let Some(t) = v.as_i64() {
        return Some(if t < 1_000_000_000_000 {
            t.saturating_mul(1000)
        } else {
            t
        });
    }
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<jiff::Timestamp>()
        .ok()
        .map(|ts| ts.as_millisecond())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_sign_in_checks_only_the_non_sensitive_marker() {
        let dir = crate::testutil::scratch("gemini-sign-in");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(google_sign_in(&dir), (false, None));
        std::fs::create_dir_all(dir.join("antigravity-cli")).unwrap();
        std::fs::write(dir.join("antigravity-cli/kvistcode-authenticated"), "").unwrap();
        assert_eq!(google_sign_in(&dir), (true, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dedicated_profiles_read_agents_md_unless_they_say_otherwise() {
        let root = crate::testutil::scratch("gemini-home");
        let _ = std::fs::remove_dir_all(&root);
        let mut account = account(&root);
        let settings = root
            .join(".gemini")
            .join("antigravity-cli")
            .join("settings.json");
        prepare_home(&account);
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(v["context"]["fileName"], serde_json::json!(CONTEXT_FILES));
        // The person's own choice and other settings stay.
        std::fs::write(
            &settings,
            r#"{"security":{"auth":{"selectedType":"oauth-personal"}},"context":{"fileName":"MINE.md"}}"#,
        )
        .unwrap();
        prepare_home(&account);
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(v["context"]["fileName"], "MINE.md");
        assert_eq!(v["security"]["auth"]["selectedType"], "oauth-personal");
        // Other settings are kept when the list is added.
        std::fs::write(&settings, r#"{"ui":{"theme":"x"}}"#).unwrap();
        prepare_home(&account);
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(v["ui"]["theme"], "x");
        assert_eq!(v["context"]["fileName"], serde_json::json!(CONTEXT_FILES));
        // A file Kvistcode can't read is left alone.
        std::fs::write(&settings, "{ // mine\n}").unwrap();
        prepare_home(&account);
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ // mine\n}");
        // The system account's ~/.gemini is never written.
        std::fs::remove_dir_all(&root).unwrap();
        account.is_system = true;
        prepare_home(&account);
        assert!(!root.exists());
    }

    fn account(home: &Path) -> Account {
        Account {
            id: "a".into(),
            provider: Provider::Gemini,
            name: "G".into(),
            color: String::new(),
            auth_kind: crate::model::AuthKind::Oauth,
            is_system: false,
            config_dir: Some(home.to_string_lossy().into_owned()),
            created_at: 0,
            status: None,
            usage: None,
            local: None,
        }
    }

    #[test]
    fn a_dedicated_account_uses_its_own_folder() {
        let mut account = account(Path::new("/profiles/g"));
        assert_eq!(
            config_dir(&account),
            Some(PathBuf::from("/profiles/g/.gemini"))
        );
        account.is_system = true;
        assert_ne!(
            config_dir(&account),
            Some(PathBuf::from("/profiles/g/.gemini"))
        );
    }

    #[test]
    fn acp_uses_a_relative_app_data_directory_for_each_profile() {
        assert_eq!(
            acp_args(Path::new("/profiles/g/.gemini")),
            [
                "acp",
                "--app_data_dir",
                "antigravity-cli",
                "--gemini-dir",
                "/profiles/g/.gemini"
            ]
        );
    }

    #[test]
    fn parses_antigravity_usage_json_and_prefers_gemini_pool() {
        let raw = r#"log line before json
{"response":{"groups":[
  {"groupId":"3p-models","displayName":"Claude and GPT models","buckets":[
    {"bucketId":"3p-5h","window":"5h","remainingFraction":0.10,"resetTime":"2026-11-23T15:00:00Z"}
  ]},
  {"groupId":"gemini-models","displayName":"Gemini Models","buckets":[
    {"bucketId":"gemini-weekly","window":"weekly","remainingFraction":0.75,"resetTime":"2026-11-28T12:00:00Z"},
    {"bucketId":"gemini-5h","window":"5h","remainingFraction":0.85,"resetTime":"2026-11-23T16:45:00Z"}
  ]}
]}}"#;
        let usage = parse_usage_output(raw).expect("parsed usage");
        let p = usage.primary.expect("primary 5h");
        assert!((p.used_percent - 15.0).abs() < 1e-6);
        assert_eq!(p.window_mins, Some(300));
        assert!(p.resets_at.is_some());
        let s = usage.secondary.expect("secondary weekly");
        assert!((s.used_percent - 25.0).abs() < 1e-6);
        assert_eq!(s.window_mins, Some(10080));
        assert!(s.resets_at.is_some());
    }

    #[test]
    fn ignores_non_quota_usage_output() {
        assert!(parse_usage_output("").is_none());
        assert!(parse_usage_output("not json").is_none());
        assert!(parse_usage_output(r#"{"response":{"groups":[]}}"#).is_none());
    }

    #[test]
    fn parses_agy_command_data_envelope_and_object_plan() {
        let raw = r#"[INFO] loaded {config}
{"type":"init","version":"1.0"}
{"status":"SUCCESS","command":{"name":"/usage","data":{
  "plan":{"name":"Google AI Pro","id":"pro"},
  "groups":[
    {"name":"Claude and GPT models","buckets":[
      {"window":"5-hour window","remaining_fraction":0.20,"reset_time":"2026-11-23T16:00:00Z"}
    ]},
    {"name":"Gemini Models","buckets":[
      {"window":"Weekly","remaining_fraction":0.92,"reset_time":"2026-11-28T00:00:00Z"},
      {"window":"5-hour window","remaining_fraction":0.85,"reset_time":"2026-11-23T15:30:00Z"}
    ]}
  ]
}}}"#;
        let usage = parse_usage_output(raw).expect("parsed command.data usage");
        assert_eq!(usage.plan.as_deref(), Some("Google AI Pro"));
        let p = usage.primary.expect("primary 5h");
        assert!((p.used_percent - 15.0).abs() < 1e-6);
        assert_eq!(p.window_mins, Some(300));
        let s = usage.secondary.expect("secondary weekly");
        assert!((s.used_percent - 8.0).abs() < 1e-6);
        assert_eq!(s.window_mins, Some(10080));
    }

    #[test]
    fn parses_plain_text_and_string_response_usage() {
        let raw = r#"{"status":"SUCCESS","response":"Claude and GPT models\n5-hour window: 10% remaining\n\nGemini Models\n5-hour window: 80% remaining\nWeekly: 35% used"}"#;
        let usage = parse_usage_output(raw).expect("parsed string response usage");
        let p = usage.primary.expect("primary 5h");
        assert!((p.used_percent - 20.0).abs() < 1e-6);
        assert_eq!(p.window_mins, Some(300));
        let s = usage.secondary.expect("secondary weekly");
        assert!((s.used_percent - 35.0).abs() < 1e-6);
        assert_eq!(s.window_mins, Some(10080));
    }
}
