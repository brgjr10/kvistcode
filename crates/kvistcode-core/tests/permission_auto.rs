//! "Run without asking": Gemini (Antigravity's "auto" mode) and Codex ("on-request") agents
//! get their approvals answered by Kvistcode, so no approval card waits on the user.
#![cfg(unix)]

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

/// Runs one "auto" agent's first turn, answering any questions; returns every approval card
/// shown and the agent's items.
async fn run_auto(provider: Provider, name: &str) -> (Vec<String>, Vec<TimelineItem>) {
    let data = common::scratch(&format!("test-auto-{name}-data"));
    let project = common::scratch(&format!("test-auto-{name}-project"));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let core = Core::open(data, Arc::new(Sink(tx))).unwrap();
    let account = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == provider && a.is_system)
        .expect("system account");
    let agent = core
        .create_agent(CreateAgentRequest {
            provider,
            account_id: account.id,
            project_path: project.to_string_lossy().into_owned(),
            prompt: "go".into(),
            title: None,
            worktree: false,
            model: None,
            group_id: None,
            custom_agent_id: None,
            permission_mode: Some("auto".into()),
            attachments: vec![],
            config_choices: Default::default(),
        })
        .await
        .unwrap();
    let mut cards = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match rx.recv().await.expect("channel open") {
                KvistcodeEvent::ItemUpserted { item } => match &item.body {
                    ItemBody::Permission {
                        request_id,
                        title,
                        resolution: None,
                        ..
                    } => {
                        cards.push(title.clone());
                        // Deny, so the turn still ends.
                        let deny = if provider == Provider::Codex {
                            "decline"
                        } else {
                            "no"
                        };
                        core.respond_permission(&agent.id, request_id, deny)
                            .await
                            .unwrap();
                    }
                    ItemBody::Question {
                        request_id,
                        fields,
                        resolution: None,
                        ..
                    } => {
                        let answer = serde_json::json!({ fields[0].id.clone(): "x" });
                        core.answer_question(
                            &agent.id,
                            request_id,
                            Some(serde_json::from_value(answer).unwrap()),
                        )
                        .await
                        .unwrap();
                    }
                    _ => {}
                },
                KvistcodeEvent::Attention {
                    reason: AttentionReason::Done,
                    ..
                } => break,
                KvistcodeEvent::Attention {
                    reason: AttentionReason::Error,
                    body,
                    ..
                } => panic!("turn failed: {body}"),
                _ => {}
            }
        }
    })
    .await
    .expect("timed out waiting for the turn");
    let items = core.items(&agent.id).unwrap();
    core.shutdown().await;
    (cards, items)
}

fn reply(items: &[TimelineItem]) -> String {
    items
        .iter()
        .filter_map(|i| match &i.body {
            ItemBody::AgentMessage { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn gemini_auto_answers_approvals_itself() {
    let mock = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_acp.py");
    std::env::set_var("KVISTCODE_GEMINI_COMMAND", format!("python3 {mock}"));
    let (cards, items) = run_auto(Provider::Gemini, "gemini").await;
    assert!(cards.is_empty(), "approval shown in auto mode: {cards:?}");
    // The mock's command ran (it fails a denied one).
    let ran = items.iter().any(|i| {
        matches!(&i.body, ItemBody::ToolCall { title, status, .. }
            if title == "Run ls" && matches!(status, ToolStatus::Completed))
    });
    assert!(ran, "the allowed command didn't complete");
    assert_eq!(reply(&items), "You said: go");
}

#[tokio::test]
async fn codex_auto_answers_approvals_itself() {
    let mock = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/mock_codex_full.py");
    std::env::set_var("KVISTCODE_CODEX_COMMAND", format!("python3 {mock}"));
    let (cards, items) = run_auto(Provider::Codex, "codex").await;
    assert!(cards.is_empty(), "approval shown in auto mode: {cards:?}");
    let text = reply(&items);
    assert!(text.starts_with("policy=on-request "), "{text}");
    // The network grant is allowed for the turn and the legacy command approved.
    assert!(
        text.contains(r#""exec": {"decision": "approved"}"#),
        "{text}"
    );
    assert!(text.contains(r#""scope": "turn""#), "{text}");
}
