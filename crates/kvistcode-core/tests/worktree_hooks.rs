//! Worktree setup & teardown hooks end to end:
//! - Untrusted repo commands pause with `NeedsTrust` and never run silently.
//! - Modifying a repo config on disk before approval invalidates the prompt (TOCTOU guard).
//! - Approving with `trust_always` copies files, runs setup, remembers the hash, and starts the agent.
//! - Failing setup surfaces redacted output and allows retry or skip.
//! - Deleting a worktree agent runs its trusted teardown command before removal.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::worktree_hooks::{ProjectWorktreeConfig, WorktreeSetupStatus};
use kvistcode_core::*;
use tokio::sync::mpsc;

struct Sink(mpsc::UnboundedSender<KvistcodeEvent>);

impl EventSink for Sink {
    fn emit(&self, event: KvistcodeEvent) {
        let _ = self.0.send(event);
    }
}

mod common;

fn temp_dir(name: &str) -> PathBuf {
    common::scratch(&format!("wt-hooks-{name}"))
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn repo(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    git(&dir, &["init", "-q"]);
    for (k, v) in [
        ("user.name", "T"),
        ("user.email", "t@t.invalid"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&dir, &["config", k, v]);
    }
    std::fs::write(dir.join("README.md"), "# test\n").unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    dir
}

async fn core_with_mock(name: &str) -> (Core, String) {
    let mock = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_acp.py").to_string();
    let (tx, rx) = mpsc::unbounded_channel();
    std::mem::forget(rx);
    let core = Core::open(temp_dir(&format!("{name}-data")), Arc::new(Sink(tx))).unwrap();
    let mut settings = core.settings().unwrap();
    settings.custom_agents = vec![CustomAgent {
        id: "mock".into(),
        name: "Mock".into(),
        command: "python3".into(),
        args: vec![mock],
        env: vec![],
        color: "#B39DDB".into(),
        enabled: true,
    }];
    core.save_settings(settings).unwrap();
    let account = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == Provider::Custom)
        .expect("system custom account");
    (core, account.id)
}

async fn wait_until<F>(timeout: Duration, mut pred: F)
where
    F: FnMut() -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if pred() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition did not become true within {timeout:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn create_req(account_id: &str, project: &Path, prompt: &str) -> CreateAgentRequest {
    CreateAgentRequest {
        provider: Provider::Custom,
        account_id: account_id.to_string(),
        project_path: project.to_string_lossy().into_owned(),
        prompt: prompt.to_string(),
        worktree: true,
        title: None,
        group_id: None,
        permission_mode: Some("auto".into()),
        custom_agent_id: Some("mock".into()),
        model: None,
        attachments: vec![],
        config_choices: Default::default(),
    }
}

#[tokio::test]
async fn untrusted_repo_hooks_wait_for_approval_then_run_and_teardown() {
    let (core, account_id) = core_with_mock("trust-flow").await;
    let project = repo("trust-flow");
    let markers = temp_dir("trust-markers");
    let setup_marker = markers.join("setup.txt");
    let teardown_marker = markers.join("teardown.txt");

    // Untracked local env file in main checkout.
    std::fs::write(project.join(".env"), "LOCAL_FLAG=ok\n").unwrap();
    std::fs::write(
        project.join(".kvistcode.json"),
        serde_json::json!({
            "worktree": {
                "copyFiles": [".env"],
                "setupCommand": format!("printf 'ran:%s' \"$KVISTCODE_WORKTREE_PATH\" > \"{}\"", setup_marker.display()),
                "teardownCommand": format!("printf 'bye:%s' \"$KVISTCODE_AGENT_ID\" > \"{}\"", teardown_marker.display()),
            }
        })
        .to_string(),
    )
    .unwrap();

    // 1. Creating a worktree agent must NOT run the untrusted repo command.
    let agent = core
        .create_agent(create_req(&account_id, &project, ""))
        .await
        .unwrap();
    assert!(agent.worktree);
    let setup = agent.worktree_setup.as_ref().expect("needs_trust state");
    assert_eq!(setup.status, WorktreeSetupStatus::NeedsTrust);
    assert_eq!(setup.source_path.as_deref(), Some(".kvistcode.json"));
    assert!(!setup_marker.exists(), "untrusted script must not execute");

    // 2. TOCTOU guard: if the repo config changes on disk before approval, approval refuses.
    std::fs::write(
        project.join(".kvistcode.json"),
        serde_json::json!({
            "worktree": {
                "copyFiles": [".env"],
                "setupCommand": format!("printf 'updated:%s' \"$KVISTCODE_WORKTREE_PATH\" > \"{}\"", setup_marker.display()),
                "teardownCommand": format!("printf 'bye:%s' \"$KVISTCODE_AGENT_ID\" > \"{}\"", teardown_marker.display()),
            }
        })
        .to_string(),
    )
    .unwrap();
    let err = core
        .approve_worktree_hooks(&agent.id, true)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("changed on disk"), "{err:#}");
    assert!(!setup_marker.exists());

    // 3. Approving the refreshed command runs copyFiles + setupCommand and trusts the hash.
    core.approve_worktree_hooks(&agent.id, true).await.unwrap();
    wait_until(Duration::from_secs(10), || {
        core.agent(&agent.id)
            .ok()
            .is_some_and(|a| a.worktree_setup.is_none())
    })
    .await;

    assert!(setup_marker.exists(), "setup command should have run");
    assert!(std::fs::read_to_string(&setup_marker)
        .unwrap()
        .starts_with("updated:"),);
    assert_eq!(
        std::fs::read_to_string(Path::new(&agent.cwd).join(".env")).unwrap(),
        "LOCAL_FLAG=ok\n"
    );

    let hooks = core.get_worktree_hooks(&project.to_string_lossy()).unwrap();
    assert!(!hooks.needs_trust);
    assert!(hooks.repo.unwrap().trusted);

    // 4. Deleting the agent runs the trusted teardown command right before removing the worktree.
    core.delete_agent(&agent.id, true, true).await.unwrap();
    assert!(
        teardown_marker.exists(),
        "trusted teardown command should run before worktree removal"
    );
    assert_eq!(
        std::fs::read_to_string(&teardown_marker).unwrap(),
        format!("bye:{}", agent.id)
    );
    assert!(!Path::new(&agent.cwd).exists());

    core.shutdown().await;
}

#[tokio::test]
async fn failing_user_setup_surfaces_redacted_output_and_can_be_skipped() {
    let (core, account_id) = core_with_mock("fail-flow").await;
    let project = repo("fail-flow");
    let key = project.to_string_lossy().into_owned();

    let mut settings = core.settings().unwrap();
    settings.project_worktrees.insert(
        key.clone(),
        ProjectWorktreeConfig {
            copy_files: vec![],
            setup_command: Some(
                "echo 'boom key=sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd' >&2; exit 7"
                    .into(),
            ),
            teardown_command: None,
        },
    );
    core.save_settings(settings).unwrap();

    let agent = core
        .create_agent(create_req(&account_id, &project, ""))
        .await
        .unwrap();

    wait_until(Duration::from_secs(10), || {
        core.agent(&agent.id).ok().is_some_and(|a| {
            a.worktree_setup
                .as_ref()
                .is_some_and(|s| s.status == WorktreeSetupStatus::Failed)
        })
    })
    .await;

    let failed = core.agent(&agent.id).unwrap();
    assert_eq!(failed.status, AgentStatus::Error);
    let wt = failed.worktree_setup.as_ref().unwrap();
    assert!(wt.error.as_deref().unwrap_or("").contains("status 7"));
    let output = wt.output.as_deref().unwrap_or("");
    assert!(output.contains("boom"));
    assert!(
        !output.contains("sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ"),
        "secret must be redacted from setup output: {output}"
    );

    // Skipping clears the failure state and lets the agent warm up cleanly.
    let skipped = core.skip_worktree_hooks(&agent.id).await.unwrap();
    assert!(skipped.worktree_setup.is_none());

    core.delete_agent(&agent.id, true, true).await.unwrap();
    core.shutdown().await;
}
