//! Codex agents get a slash menu (`/compact` and skills); `/compact` and `/skill` reach
//! Codex as the calls it understands.

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::*;
use tokio::sync::mpsc;

struct Sink(mpsc::UnboundedSender<KvistcodeEvent>);

impl EventSink for Sink {
    fn emit(&self, event: KvistcodeEvent) {
        let _ = self.0.send(event);
    }
}

mod common;

async fn next_done(rx: &mut mpsc::UnboundedReceiver<KvistcodeEvent>) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match rx.recv().await.expect("channel open") {
                KvistcodeEvent::Attention {
                    reason: AttentionReason::Done,
                    ..
                } => return,
                KvistcodeEvent::Attention {
                    reason: AttentionReason::Error,
                    ..
                } => panic!("the turn failed"),
                _ => {}
            }
        }
    })
    .await
    .expect("the turn never finished");
}

fn messages(core: &Core, id: &str) -> Vec<String> {
    core.items(id)
        .unwrap()
        .into_iter()
        .filter_map(|i| match i.body {
            ItemBody::AgentMessage { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn codex_lists_commands_and_runs_compact_and_skills() {
    let mock = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_codex_commands.py");
    std::env::set_var("KVISTCODE_CODEX_COMMAND", format!("python3 {mock}"));
    let data = common::scratch("test-codex-commands-data");
    let project = common::scratch("test-codex-commands-project");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let core = Core::open(data, Arc::new(Sink(tx))).unwrap();
    let account = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == Provider::Codex && a.is_system)
        .unwrap();
    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Codex,
            account_id: account.id,
            project_path: project.to_string_lossy().into_owned(),
            prompt: "go".into(),
            title: None,
            worktree: false,
            model: None,
            group_id: None,
            custom_agent_id: None,
            permission_mode: None,
            attachments: vec![],
            config_choices: Default::default(),
        })
        .await
        .unwrap();
    next_done(&mut rx).await;

    // The skills list is fetched in the background.
    let mut names: Vec<String> = Vec::new();
    for _ in 0..80 {
        names = core
            .agent(&agent.id)
            .unwrap()
            .available_commands
            .into_iter()
            .map(|c| c.name)
            .collect();
        if names.len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(names, ["compact", "deploy"]);

    core.send_prompt(&agent.id, "/compact", vec![])
        .await
        .unwrap();
    next_done(&mut rx).await;
    core.send_prompt(&agent.id, "/deploy to prod", vec![])
        .await
        .unwrap();
    next_done(&mut rx).await;

    let said = messages(&core, &agent.id);
    assert!(said.contains(&"compacted".to_string()), "{said:?}");
    assert!(
        said.contains(&"input=text,skill text=$deploy to prod".to_string()),
        "{said:?}"
    );
    core.shutdown().await;
}
