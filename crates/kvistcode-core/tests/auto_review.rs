//! End-to-end integration tests for automatic cross-provider review ("Review before Needs you").

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::auto_review::{AutoReviewConfig, AutoReviewSeverity, AutoReviewStatus};
use kvistcode_core::*;
use tokio::sync::mpsc;

mod common;

struct Sink(mpsc::UnboundedSender<KvistcodeEvent>);

impl EventSink for Sink {
    fn emit(&self, event: KvistcodeEvent) {
        let _ = self.0.send(event);
    }
}

fn init_git_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("app.rs"), "pub fn version() -> u32 { 0 }\n").unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["add", "."]);
    git(&["commit", "-m", "init"]);
}

fn mock_script() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/mock_reviewer.py")
        .to_string_lossy()
        .into_owned()
}

fn read_jsonl(path: &Path) -> Vec<serde_json::Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

async fn wait_for<F>(timeout: Duration, mut pred: F)
where
    F: FnMut() -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if pred() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    panic!("condition was not met within {timeout:?}");
}

#[tokio::test]
async fn auto_review_runs_on_turn_end_dedups_and_caps_auto_fix_at_once() {
    let base = common::scratch("test-auto-review");
    let repo = base.join("repo");
    init_git_repo(&repo);
    let worker_log = base.join("worker.jsonl");
    let review_log = base.join("review.jsonl");

    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let core = Core::open(base.join("data"), Arc::new(Sink(tx))).unwrap();

    let script = mock_script();
    let repo_str = repo.to_string_lossy().into_owned();
    core.update_settings(|s| {
        s.custom_agents = vec![
            CustomAgent {
                id: "worker-agent".into(),
                name: "Mock Worker".into(),
                command: "python3".into(),
                args: vec![script.clone()],
                env: vec![
                    KeyValue {
                        name: "MOCK_ROLE".into(),
                        value: "worker".into(),
                    },
                    KeyValue {
                        name: "MOCK_WORKER_LOG".into(),
                        value: worker_log.to_string_lossy().into_owned(),
                    },
                ],
                color: "#aaa".into(),
                enabled: true,
            },
            CustomAgent {
                id: "reviewer-agent".into(),
                name: "Mock Reviewer".into(),
                command: "python3".into(),
                args: vec![script.clone()],
                env: vec![
                    KeyValue {
                        name: "MOCK_ROLE".into(),
                        value: "reviewer".into(),
                    },
                    KeyValue {
                        name: "MOCK_REVIEW_LOG".into(),
                        value: review_log.to_string_lossy().into_owned(),
                    },
                ],
                color: "#bbb".into(),
                enabled: true,
            },
        ];
        s.project_auto_review.insert(
            repo_str.clone(),
            AutoReviewConfig {
                enabled: true,
                provider: Some(Provider::Custom),
                account_id: None,
                custom_agent_id: Some("reviewer-agent".into()),
                model: None,
                auto_fix_critical: true,
            },
        );
    })
    .unwrap();

    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Custom,
            account_id: "system-custom".into(),
            project_path: repo_str.clone(),
            prompt: "Change version".into(),
            title: Some("Update version".into()),
            worktree: true,
            model: None,
            group_id: None,
            custom_agent_id: Some("worker-agent".into()),
            permission_mode: Some("auto".into()),
            attachments: vec![],
            config_choices: Default::default(),
        })
        .await
        .unwrap();

    // Expect:
    // 1. Turn 1 edits app.rs -> auto-review runs -> finds critical -> auto_fix_sent=true -> sends 1 fix prompt
    // 2. Turn 2 (auto-fix) edits app.rs -> auto-review runs on new diff -> sees critical again,
    //    but auto_fix_sent is ALREADY true so it NEVER sends a 3rd turn!
    wait_for(Duration::from_secs(15), || {
        let reviews = read_jsonl(&review_log);
        let a = core.agent(&agent.id).unwrap();
        reviews.len() == 2
            && a.status == AgentStatus::Idle
            && a.auto_review_state
                .as_ref()
                .is_some_and(|s| s.status == AutoReviewStatus::Findings && s.auto_fix_sent)
    })
    .await;

    // Wait briefly to ensure no 3rd turn is spawned.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let worker_calls = read_jsonl(&worker_log);
    assert_eq!(
        worker_calls.len(),
        2,
        "auto_fix_critical must send at most 1 automatic fix turn: {worker_calls:?}"
    );
    assert!(worker_calls[1]["text"]
        .as_str()
        .unwrap()
        .contains("Hardcoded version number"));

    let review_calls = read_jsonl(&review_log);
    assert_eq!(review_calls.len(), 2);
    // Reviewer ran in an isolated temp dir (not the agent's worktree!) and its permission request was denied.
    for call in &review_calls {
        assert_ne!(
            call["cwd"].as_str().unwrap(),
            agent.cwd,
            "reviewer must never run directly inside the agent's worktree"
        );
        assert_eq!(call["permOutcome"]["optionId"].as_str(), Some("deny"));
    }

    // Verify timeline notice was emitted.
    let items = core.items(&agent.id).unwrap();
    assert!(items.iter().any(|i| matches!(
        &i.body,
        ItemBody::Notice { text, .. } if text.contains("Cross-provider review")
    )));

    // Turn 3 with NO_EDIT keeps the exact same diff fingerprint: reviewer must NOT re-run.
    core.send_prompt(&agent.id, "NO_EDIT just summarize", vec![])
        .await
        .unwrap();
    wait_for(Duration::from_secs(10), || {
        read_jsonl(&worker_log).len() == 3
            && core.agent(&agent.id).unwrap().status == AgentStatus::Idle
    })
    .await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        read_jsonl(&review_log).len(),
        2,
        "identical diff fingerprint must not trigger a duplicate review"
    );

    // Dismissing the finding transitions status to Passed.
    let state = core.agent(&agent.id).unwrap().auto_review_state.unwrap();
    assert_eq!(state.findings.len(), 1);
    assert_eq!(state.findings[0].severity, AutoReviewSeverity::Critical);
    let after_dismiss = core
        .dismiss_auto_review_finding(&agent.id, &state.findings[0].id)
        .unwrap();
    assert_eq!(
        after_dismiss.auto_review_state.as_ref().unwrap().status,
        AutoReviewStatus::Passed
    );
    assert!(after_dismiss
        .auto_review_state
        .as_ref()
        .unwrap()
        .findings
        .is_empty());

    // Manual `run_auto_review` runs even though the diff fingerprint hasn't changed, and does NOT auto-send.
    let started = core.run_auto_review(&agent.id).await.unwrap();
    assert_eq!(
        started.auto_review_state.as_ref().unwrap().status,
        AutoReviewStatus::Running
    );
    wait_for(Duration::from_secs(10), || {
        core.agent(&agent.id)
            .unwrap()
            .auto_review_state
            .as_ref()
            .is_some_and(|s| s.status == AutoReviewStatus::Findings)
    })
    .await;
    assert_eq!(read_jsonl(&review_log).len(), 3);
    assert_eq!(
        read_jsonl(&worker_log).len(),
        3,
        "manual run_auto_review must not auto-send a fix prompt"
    );

    // `send_auto_review_findings` sends chosen findings to the main agent and clears them.
    let finding_id = core
        .agent(&agent.id)
        .unwrap()
        .auto_review_state
        .unwrap()
        .findings[0]
        .id
        .clone();
    let after_send = core
        .send_auto_review_findings(&agent.id, vec![finding_id])
        .await
        .unwrap();
    assert!(after_send
        .auto_review_state
        .as_ref()
        .unwrap()
        .findings
        .is_empty());
    wait_for(Duration::from_secs(10), || {
        read_jsonl(&worker_log).len() == 4
            && core.agent(&agent.id).unwrap().status == AgentStatus::Idle
    })
    .await;

    // Per-agent override can disable auto-review even when project default is enabled.
    let disabled = core
        .set_agent_auto_review(
            &agent.id,
            Some(AutoReviewConfig {
                enabled: false,
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(
        disabled.auto_review.as_ref().map(|c| c.enabled),
        Some(false)
    );

    core.shutdown().await;
}

#[tokio::test]
async fn auto_fix_respects_turn_budget_cap_and_relocate_moves_project_config() {
    let base = common::scratch("test-auto-review-budget");
    let repo = base.join("repo");
    init_git_repo(&repo);
    let worker_log = base.join("worker.jsonl");
    let review_log = base.join("review.jsonl");

    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let core = Core::open(base.join("data"), Arc::new(Sink(tx))).unwrap();

    let script = mock_script();
    let repo_str = repo.to_string_lossy().into_owned();
    core.update_settings(|s| {
        // Cap agent at 1 completed turn (even under Notify action, auto-review must not spend past cap).
        s.agent_budget_turns = Some(1);
        s.budget_action = budget::BudgetAction::Notify;
        s.custom_agents = vec![
            CustomAgent {
                id: "w".into(),
                name: "Worker".into(),
                command: "python3".into(),
                args: vec![script.clone()],
                env: vec![
                    KeyValue {
                        name: "MOCK_ROLE".into(),
                        value: "worker".into(),
                    },
                    KeyValue {
                        name: "MOCK_WORKER_LOG".into(),
                        value: worker_log.to_string_lossy().into_owned(),
                    },
                ],
                color: "#aaa".into(),
                enabled: true,
            },
            CustomAgent {
                id: "r".into(),
                name: "Reviewer".into(),
                command: "python3".into(),
                args: vec![script.clone()],
                env: vec![
                    KeyValue {
                        name: "MOCK_ROLE".into(),
                        value: "reviewer".into(),
                    },
                    KeyValue {
                        name: "MOCK_REVIEW_LOG".into(),
                        value: review_log.to_string_lossy().into_owned(),
                    },
                ],
                color: "#bbb".into(),
                enabled: true,
            },
        ];
        s.project_auto_review.insert(
            repo_str.clone(),
            AutoReviewConfig {
                enabled: true,
                provider: Some(Provider::Custom),
                account_id: None,
                custom_agent_id: Some("r".into()),
                model: None,
                auto_fix_critical: true,
            },
        );
    })
    .unwrap();

    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Custom,
            account_id: "system-custom".into(),
            project_path: repo_str.clone(),
            prompt: "Do work".into(),
            title: Some("Budgeted".into()),
            worktree: true,
            model: None,
            group_id: None,
            custom_agent_id: Some("w".into()),
            permission_mode: Some("auto".into()),
            attachments: vec![],
            config_choices: Default::default(),
        })
        .await
        .unwrap();

    wait_for(Duration::from_secs(10), || {
        read_jsonl(&worker_log).len() == 1
            && core.agent(&agent.id).unwrap().status == AgentStatus::Idle
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Because agent already hit its 1-turn cap, neither auto-review nor auto-fix runs past the cap.
    assert_eq!(read_jsonl(&worker_log).len(), 1);
    assert_eq!(read_jsonl(&review_log).len(), 0);

    // Relocating the project moves `project_auto_review` along with it.
    let moved = base.join("repo-moved");
    std::fs::create_dir_all(&moved).unwrap();
    let moved_str = moved.canonicalize().unwrap().to_string_lossy().into_owned();
    core.relocate_project(&repo_str, &moved_str).await.unwrap();
    let s = core.settings().unwrap();
    assert!(!s.project_auto_review.contains_key(&repo_str));
    assert_eq!(
        s.project_auto_review
            .get(&moved_str)
            .and_then(|c| c.custom_agent_id.as_deref()),
        Some("r")
    );

    core.shutdown().await;
}
