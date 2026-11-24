//! OpenCode and Cursor are built-in ACP providers: a system account exists for each, an
//! agent starts through `KVISTCODE_<PROVIDER>_COMMAND`, the model list reaches the agent as
//! a picker, and token counts come from the agent's own report (nothing is invented).

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::*;

mod common;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: KvistcodeEvent) {}
}

const MOCK: &str = r#"#!/usr/bin/env python3
import json, sys

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    mid = msg.get("id")
    method = msg.get("method")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "protocolVersion": 1,
            "agentCapabilities": {"promptCapabilities": {"image": True}},
            "agentInfo": {"name": "mock-acp-cli", "version": "1.0.0"}
        }})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "sessionId": "sess-1",
            "models": {"currentModelId": "opencode/big-pickle", "availableModels": [
                {"modelId": "opencode/big-pickle", "name": "Big Pickle"},
                {"modelId": "anthropic/claude-sonnet", "name": "Claude Sonnet"}
            ]}
        }})
    elif method == "session/prompt":
        sid = msg["params"]["sessionId"]
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {"sessionUpdate": "agent_message_chunk",
                       "content": {"type": "text", "text": "Done."}}}})
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "stopReason": "end_turn",
            "_meta": {"totalTokens": 900, "usage": {"inputTokens": 800, "outputTokens": 100}}
        }})
"#;

async fn wait_idle(core: &Core, id: &str) -> Agent {
    for _ in 0..200 {
        let a = core.agent(id).unwrap();
        if matches!(a.status, AgentStatus::Idle | AgentStatus::Error) {
            return a;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("agent {id} did not settle");
}

#[tokio::test]
async fn opencode_and_cursor_agents_run_through_their_system_accounts() {
    let root = common::scratch("opencode-cursor");
    let data = root.join("data");
    let proj = root.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let mock = root.join("mock_acp_cli.py");
    std::fs::write(&mock, MOCK).unwrap();
    for var in ["KVISTCODE_OPENCODE_COMMAND", "KVISTCODE_CURSOR_COMMAND"] {
        std::env::set_var(var, format!("python3 {}", mock.display()));
    }

    let core = Core::open(data, Arc::new(Quiet)).unwrap();
    for provider in [Provider::Opencode, Provider::Cursor] {
        let account = core
            .accounts()
            .unwrap()
            .into_iter()
            .find(|a| a.provider == provider && a.is_system)
            .expect("system account for the provider");
        let agent = core
            .create_agent(CreateAgentRequest {
                provider,
                account_id: account.id,
                custom_agent_id: None,
                project_path: proj.to_string_lossy().into_owned(),
                worktree: false,
                title: Some("t".into()),
                prompt: "go".into(),
                attachments: Vec::new(),
                group_id: None,
                model: None,
                permission_mode: None,
                config_choices: Default::default(),
            })
            .await
            .unwrap();
        let settled = wait_idle(&core, &agent.id).await;
        assert_eq!(settled.status, AgentStatus::Idle, "{provider:?}");
        assert_eq!(settled.provider, provider);
        assert_eq!(settled.tokens_used, 900);
        let model = settled
            .config_options
            .iter()
            .find(|o| o.id == "model")
            .expect("model picker");
        assert_eq!(model.options.len(), 2);
    }

    // Old data: provider names written before these existed still load, and the new
    // ones round-trip through their stored form.
    assert_eq!(Provider::parse("opencode"), Some(Provider::Opencode));
    assert_eq!(Provider::parse("cursor"), Some(Provider::Cursor));
    assert_eq!(
        serde_json::from_str::<Provider>("\"gemini\"").unwrap(),
        Provider::Gemini
    );

    core.shutdown().await;
    let _ = std::fs::remove_dir_all(&root);
}
