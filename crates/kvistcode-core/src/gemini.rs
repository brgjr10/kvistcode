//! Gemini CLI specifics: where an account's sign-in lives, and the Google sign-in.
//!
//! Gemini CLI keeps everything under `$GEMINI_CLI_HOME/.gemini` (`~/.gemini` without
//! it): `oauth_creds.json` after a Google sign-in, `google_accounts.json` naming the
//! signed-in address, and `settings.json` with the chosen sign-in method. It has no
//! `login` command: the Google sign-in runs through ACP's `authenticate`, which (in
//! ACP mode) opens the browser straight away and saves the choice. [`login_command`]
//! runs a tiny Node script that does just that and exits.

use std::path::{Path, PathBuf};

use crate::model::{Account, Provider};
use crate::providers;

/// Set for every Gemini process: secrets go to an encrypted file in the account's own
/// folder, never the OS keychain (which Gemini CLI tries first when its keytar module
/// loads). The keychain is shared by every profile, and touching it can pop a "create
/// a keyring" dialog.
pub const FORCE_FILE_STORAGE: (&str, &str) = ("GEMINI_FORCE_FILE_STORAGE", "true");

/// The Node script behind the sign-in: starts `gemini --acp`, asks it to sign in with
/// Google, prints how it went and exits (0 when signed in).
const LOGIN_SCRIPT: &str = r#"// Kvistcode: signs Gemini CLI in with Google, then exits.
const { spawn } = require("node:child_process");
const cli = process.argv[2];
const js = /\.[cm]?js$/i.test(cli);
const win = process.platform === "win32";
// Its own process group: Gemini CLI restarts itself in a child process, and both go.
const child = spawn(js ? process.execPath : cli, js ? [cli, "--acp"] : ["--acp"], {
  stdio: ["pipe", "pipe", "inherit"],
  shell: !js && win,
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
child.on("error", (e) => finish(1, "Couldn't start Gemini CLI: " + e.message));
child.on("exit", () => finish(1, "Gemini CLI stopped before the sign-in finished."));
child.stdout.on("data", (chunk) => {
  buf += chunk;
  let i;
  while ((i = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    let msg;
    try { msg = JSON.parse(line); } catch { continue; }
    if (msg.id === 1) {
      if (msg.error) return finish(1, "Gemini CLI didn't start: " + msg.error.message);
      console.log("Signing in to Gemini with your Google account.");
      console.log("A browser window opens; finish signing in there.\n");
      send(2, "authenticate", { methodId: "oauth-personal" });
    } else if (msg.id === 2) {
      if (msg.error) return finish(1, "Sign-in didn't finish: " + msg.error.message);
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
    let path = dir.join("settings.json");
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
        return;
    }
    context.insert("fileName".into(), serde_json::json!(CONTEXT_FILES));
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(text) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(&path, text);
        }
    }
}

/// The script Gemini CLI runs from, so Node can start it directly. npm puts a symlink
/// (Unix) or a `.cmd` shim (Windows) on PATH; both lead to the package's `gemini.js`.
/// Falls back to the CLI itself (a native or wrapped install).
fn cli_entry(cli: &Path) -> PathBuf {
    if let Ok(real) = std::fs::canonicalize(cli) {
        if real.extension().is_some_and(|e| e == "js") {
            return real;
        }
    }
    if let Some(dir) = cli.parent() {
        let js = dir
            .join("node_modules")
            .join(providers::GEMINI_PKG)
            .join("bundle")
            .join("gemini.js");
        if js.is_file() {
            return js;
        }
    }
    cli.to_path_buf()
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
            cli_entry(&cli).to_string_lossy().into_owned(),
        ],
    ))
}

/// Signed in with Google: `oauth_creds.json` holds a token. Returns the address from
/// `google_accounts.json` when there is one. Tokens are read, never kept or returned.
pub fn google_sign_in(dir: &Path) -> (bool, Option<String>) {
    let creds: serde_json::Value = std::fs::read(dir.join("oauth_creds.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let signed_in = ["refresh_token", "access_token"]
        .iter()
        .any(|k| creds[k].as_str().is_some_and(|t| !t.is_empty()));
    let email = std::fs::read(dir.join("google_accounts.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["active"].as_str().map(str::to_string))
        .filter(|e| !e.is_empty());
    (signed_in, email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_sign_in_reads_the_token_and_address_only() {
        let dir = crate::testutil::scratch("gemini-sign-in");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(google_sign_in(&dir), (false, None));
        std::fs::write(dir.join("oauth_creds.json"), r#"{"access_token":""}"#).unwrap();
        assert_eq!(google_sign_in(&dir), (false, None));
        std::fs::write(
            dir.join("oauth_creds.json"),
            r#"{"refresh_token":"1//x","expiry_date":1}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("google_accounts.json"),
            r#"{"active":"me@example.com","old":[]}"#,
        )
        .unwrap();
        assert_eq!(
            google_sign_in(&dir),
            (true, Some("me@example.com".to_string()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dedicated_profiles_read_agents_md_unless_they_say_otherwise() {
        let root = crate::testutil::scratch("gemini-home");
        let _ = std::fs::remove_dir_all(&root);
        let mut account = account(&root);
        let settings = root.join(".gemini").join("settings.json");
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
}
