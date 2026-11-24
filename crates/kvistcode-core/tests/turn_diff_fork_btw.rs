//! End-to-end integration tests for per-turn diffs, checkpoint forking, and `/btw` side-chat.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::*;
use tokio::sync::mpsc;

mod common;

struct Sink(mpsc::UnboundedSender<KvistcodeEvent>);

impl EventSink for Sink {
    fn emit(&self, event: KvistcodeEvent) {
        let _ = self.0.send(event);
    }
}

fn sh_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@kvistcode.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git command");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    sh_git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "base\n").unwrap();
    sh_git(dir, &["add", "-A"]);
    sh_git(dir, &["commit", "-q", "-m", "initial"]);
}

async fn next_matching(
    rx: &mut mpsc::UnboundedReceiver<KvistcodeEvent>,
    mut f: impl FnMut(&KvistcodeEvent) -> bool,
) -> KvistcodeEvent {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let ev = rx.recv().await.expect("event channel open");
            if f(&ev) {
                return ev;
            }
        }
    })
    .await
    .expect("timed out waiting for expected event")
}

fn open_core(
    prefix: &str,
) -> (
    PathBuf,
    Core,
    mpsc::UnboundedReceiver<KvistcodeEvent>,
    Account,
) {
    let mock = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_acp.py");
    std::env::set_var("KVISTCODE_GROK_COMMAND", format!("python3 {mock}"));

    let root = common::scratch(prefix);
    let data = root.join("data");
    let project = root.join("repo");
    init_repo(&project);

    let (tx, rx) = mpsc::unbounded_channel();
    let core = Core::open(data, Arc::new(Sink(tx))).unwrap();
    let account = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == Provider::Grok && a.is_system)
        .expect("system grok account");
    (project, core, rx, account)
}

#[tokio::test]
async fn turn_diff_and_checkpoint_fork_end_to_end() {
    let (project, core, mut rx, account) = open_core("turn-diff-fork");

    // Turn 1: starts from "base\n".
    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Grok,
            account_id: account.id.clone(),
            project_path: project.to_string_lossy().into_owned(),
            prompt: "SUBAGENT turn one".into(),
            title: Some("Main task".into()),
            worktree: true,
            model: None,
            group_id: None,
            custom_agent_id: None,
            permission_mode: None,
            attachments: vec![],
            config_choices: Default::default(),
        })
        .await
        .unwrap();

    next_matching(&mut rx, |e| {
        matches!(
            e,
            KvistcodeEvent::Attention {
                agent_id,
                reason: AttentionReason::Done,
                ..
            } if agent_id == &agent.id
        )
    })
    .await;

    // Simulate edits produced during turn 1.
    let cwd = PathBuf::from(&agent.cwd);
    std::fs::write(cwd.join("a.txt"), "turn1\n").unwrap();
    std::fs::write(cwd.join("t1.txt"), "from turn 1\n").unwrap();

    // Turn 2: snapshots the turn-1 files into checkpoint #2 before running.
    core.send_prompt(&agent.id, "SUBAGENT turn two", vec![])
        .await
        .unwrap();
    next_matching(&mut rx, |e| {
        matches!(
            e,
            KvistcodeEvent::Attention {
                agent_id,
                reason: AttentionReason::Done,
                ..
            } if agent_id == &agent.id
        )
    })
    .await;

    // Simulate edits produced during turn 2.
    std::fs::write(cwd.join("a.txt"), "turn2\n").unwrap();
    std::fs::write(cwd.join("t2.txt"), "from turn 2\n").unwrap();

    let ck_list = core.list_checkpoints(&agent.id).await.unwrap();
    assert!(ck_list.available);
    assert_eq!(ck_list.checkpoints.len(), 2);
    let ck1 = &ck_list.checkpoints[0];
    let ck2 = &ck_list.checkpoints[1];

    // Turn 1 diff: ck1..ck2 (only turn 1's changes, not turn 2's).
    let d1 = core.turn_diff(&agent.id, &ck1.id).await.unwrap();
    assert_eq!(d1.from_seq, 1);
    assert_eq!(d1.to_seq, Some(2));
    assert!(d1.files.iter().any(|f| f.path == "a.txt"));
    assert!(d1
        .files
        .iter()
        .any(|f| f.path == "t1.txt" && f.status == "added"));
    assert!(!d1.files.iter().any(|f| f.path == "t2.txt"));
    assert!(d1.patch.contains("+turn1"));
    assert!(!d1.patch.contains("turn2"));

    // Turn 2 diff: ck2..current working tree.
    let d2 = core.turn_diff(&agent.id, &ck2.id).await.unwrap();
    assert_eq!(d2.from_seq, 2);
    assert_eq!(d2.to_seq, None);
    assert!(d2.files.iter().any(|f| f.path == "a.txt"));
    assert!(d2
        .files
        .iter()
        .any(|f| f.path == "t2.txt" && f.status == "added"));
    assert!(d2.patch.contains("+turn2"));

    // Fork from checkpoint #2 (which had turn 1's files, before turn 2 ran).
    let forked = core
        .fork_agent(
            &agent.id,
            Some(&ck2.id),
            ForkAgentRequest {
                instruction: Some("SUBAGENT explore fork".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(forked.worktree);
    assert_ne!(forked.cwd, agent.cwd);
    assert!(forked.branch.is_some());
    assert_eq!(forked.forked_from.as_deref(), Some(agent.id.as_str()));

    let fork_cwd = PathBuf::from(&forked.cwd);
    assert_eq!(
        std::fs::read_to_string(fork_cwd.join("a.txt")).unwrap(),
        "turn1\n"
    );
    assert_eq!(
        std::fs::read_to_string(fork_cwd.join("t1.txt")).unwrap(),
        "from turn 1\n"
    );
    assert!(
        !fork_cwd.join("t2.txt").exists(),
        "fork at ck2 must not contain files created in turn 2"
    );

    next_matching(&mut rx, |e| {
        matches!(
            e,
            KvistcodeEvent::Attention {
                agent_id,
                reason: AttentionReason::Done,
                ..
            } if agent_id == &forked.id
        )
    })
    .await;

    // Deleting an unmodified fork without `force` must not falsely report its starting
    // snapshot commit as unsaved work.
    core.delete_agent(&forked.id, true, false).await.unwrap();
    core.shutdown().await;
}

#[tokio::test]
async fn side_chat_btw_runs_without_interrupting_active_turn() {
    let (project, core, mut rx, account) = open_core("side-chat-btw");

    // A regular prompt pauses at `session/request_permission` in mock_acp.py,
    // keeping the main agent actively mid-turn.
    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Grok,
            account_id: account.id.clone(),
            project_path: project.to_string_lossy().into_owned(),
            prompt: "main turn waiting for approval".into(),
            title: Some("Busy agent".into()),
            worktree: true,
            model: None,
            group_id: None,
            custom_agent_id: None,
            permission_mode: Some("ask".into()),
            attachments: vec![],
            config_choices: Default::default(),
        })
        .await
        .unwrap();

    let perm_ev = next_matching(&mut rx, |e| {
        matches!(
            e,
            KvistcodeEvent::ItemUpserted { item }
                if item.agent_id == agent.id
                    && matches!(item.body, ItemBody::Permission { resolution: None, .. })
        )
    })
    .await;
    let KvistcodeEvent::ItemUpserted { item } = perm_ev else {
        unreachable!();
    };
    let ItemBody::Permission {
        request_id,
        options,
        ..
    } = item.body
    else {
        unreachable!();
    };

    // Main agent is mid-turn waiting for permission. Run `/btw` right now.
    let initial = core
        .ask_side_question(&agent.id, "SUBAGENT quick side question")
        .await
        .unwrap();
    assert_eq!(initial.status, SideChatStatus::Running);
    assert_eq!(initial.agent_id, agent.id);

    let done_ev = next_matching(&mut rx, |e| {
        matches!(
            e,
            KvistcodeEvent::SideChatUpdated { entry }
                if entry.id == initial.id && entry.status != SideChatStatus::Running
        )
    })
    .await;
    let KvistcodeEvent::SideChatUpdated { entry } = done_ev else {
        unreachable!();
    };
    assert_eq!(entry.status, SideChatStatus::Done, "err: {:?}", entry.error);
    assert!(
        entry.answer.contains("Done with the sub-agent"),
        "unexpected answer: {:?}",
        entry.answer
    );

    // Main agent was neither interrupted nor had anything added to its queue.
    let still_busy = core.agent(&agent.id).unwrap();
    assert_eq!(still_busy.status, AgentStatus::NeedsPermission);
    assert!(still_busy.queue.is_empty());

    // Approve the main turn and confirm it still finishes cleanly.
    let allow_id = options[0].id.clone();
    core.respond_permission(&agent.id, &request_id, &allow_id)
        .await
        .unwrap();
    next_matching(&mut rx, |e| {
        matches!(
            e,
            KvistcodeEvent::Attention {
                agent_id,
                reason: AttentionReason::Done,
                ..
            } if agent_id == &agent.id
        )
    })
    .await;

    core.shutdown().await;
}
