//! Google Antigravity CLI paths and Google sign-in.
//!
//! Antigravity accepts explicit app-data and Gemini-data directories. Google sign-in runs
//! through ACP's `authenticate`, which opens a browser and saves the choice.

use std::path::{Path, PathBuf};

use crate::model::{Account, Provider};
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
}
