//! Feature parity tests across Gemini (Antigravity), Grok, Codex and custom ACP agents:
//! sub-agent launches and nested tool calls, Grok `_meta` token/context metrics, active
//! Codex `account/rateLimits/read` on connect, and rate-limit fallback windows on limit hit.

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::*;

mod common;

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: KvistcodeEvent) {}
}

fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

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
async fn gemini_and_grok_subagents_meta_usage_and_rate_limits_work_end_to_end() {
    let root = common::scratch("provider-parity");
    let data = root.join("data");
    let proj = root.join("proj");
    std::fs::create_dir_all(&proj).unwrap();

    // Mock ACP agent that emits Antigravity `invoke_subagent`, nested `parentToolCallId`,
    // Grok-style `params._meta` tokens/inputTokens, and Antigravity quota in `usage_update`.
    let mock = write_script(
        &root,
        "mock_parity_acp.py",
        r#"#!/usr/bin/env python3
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
            "agentInfo": {"name": "mock-parity", "version": "1.0.0"}
        }})
    elif method == "session/new":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "sessionId": "sess-parity",
            "models": {"currentModelId": "gemini-3-pro", "availableModels": [
                {"modelId": "gemini-3-pro", "name": "Gemini 3 Pro"}
            ]}
        }})
    elif method == "session/prompt":
        sid = msg["params"]["sessionId"]
        # Launch an Antigravity sub-agent
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "sub_1",
                "title": "invoke_subagent",
                "status": "in_progress",
                "rawInput": {
                    "TypeName": "research",
                    "Role": "Codebase Researcher",
                    "Prompt": "Find all providers",
                    "Model": "inherit"
                }
            }
        }})
        # Nested child call under that sub-agent (top-level parentToolCallId)
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "child_1",
                "parentToolCallId": "sub_1",
                "title": "Read src/lib.rs",
                "kind": "read",
                "status": "completed"
            }
        }})
        # Complete the sub-agent with async conversationId stub
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "sub_1",
                "status": "completed",
                "rawOutput": {"conversationId": "conv-99"}
            }
        }})
        # Grok-style params._meta token/context chunk + Antigravity quota in usage_update
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": "All set."}
            },
            "_meta": {
                "totalTokens": 4200,
                "usage": {"inputTokens": 3800, "outputTokens": 400}
            }
        }})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": sid,
            "update": {
                "sessionUpdate": "usage_update",
                "used": 3800,
                "size": 1000000,
                "_meta": {
                    "quota": {
                        "groups": [{
                            "groupId": "gemini-models",
                            "buckets": [
                                {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.7, "resetTime": "2099-01-01T00:00:00Z"}
                            ]
                        }]
                    }
                }
            }
        }})
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "stopReason": "end_turn",
            "_meta": {"totalTokens": 4200, "usage": {"inputTokens": 3800, "outputTokens": 400}}
        }})
"#,
    );

    let core = Core::open(data, Arc::new(Quiet)).unwrap();
    let custom = CustomAgent {
        id: "parity-acp".into(),
        name: "Parity ACP".into(),
        command: "python3".into(),
        args: vec![mock.to_string_lossy().into_owned()],
        env: Vec::new(),
        color: "#4285F4".into(),
        enabled: true,
    };
    core.update_settings(|s| s.custom_agents.push(custom))
        .unwrap();

    let custom_acc = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == Provider::Custom)
        .unwrap();

    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Custom,
            account_id: custom_acc.id,
            custom_agent_id: Some("parity-acp".into()),
            project_path: proj.to_string_lossy().into_owned(),
            worktree: false,
            title: Some("Parity".into()),
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
    assert_eq!(settled.status, AgentStatus::Idle);
    assert_eq!(settled.tokens_used, 4200);
    assert_eq!(settled.context_used, Some(3800));
    assert_eq!(settled.context_size, Some(1_000_000));

    let items = core.items(&agent.id).unwrap();
    let sub = items.iter().find_map(|i| match &i.body {
        ItemBody::ToolCall {
            tool_call_id,
            subagent: Some(sa),
            ..
        } if tool_call_id == "sub_1" => Some(sa),
        _ => None,
    });
    let sa = sub.expect("invoke_subagent recorded as SubAgent");
    assert_eq!(sa.kind.as_deref(), Some("research"));
    assert_eq!(sa.description.as_deref(), Some("Codebase Researcher"));
    assert!(sa.background);

    let child_parent = items.iter().find_map(|i| match &i.body {
        ItemBody::ToolCall {
            tool_call_id,
            parent_tool_call_id,
            ..
        } if tool_call_id == "child_1" => parent_tool_call_id.clone(),
        _ => None,
    });
    assert_eq!(child_parent.as_deref(), Some("sub_1"));

    // Usage quota snapshot propagated to the agent's account.
    let acct = core.account(&settled.account_id).unwrap();
    let usage = acct.usage.expect("account usage recorded");
    assert!((usage.primary.unwrap().used_percent - 30.0).abs() < 1e-6);

    core.shutdown().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn codex_reads_rate_limits_immediately_on_connect() {
    let root = common::scratch("codex-limits-read");
    let data = root.join("data");
    let proj = root.join("proj");
    std::fs::create_dir_all(&proj).unwrap();

    let mock = write_script(
        &root,
        "mock_codex_limits.py",
        r#"#!/usr/bin/env python3
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
        send({"jsonrpc": "2.0", "id": mid, "result": {"userAgent": "mock"}})
    elif method == "thread/start":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "thread": {"id": "thr-1"}, "model": "gpt-5-codex", "approvalPolicy": "on-request"
        }})
    elif method == "model/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"data": []}})
    elif method == "account/rateLimits/read":
        send({"jsonrpc": "2.0", "id": mid, "result": {
            "rateLimitsByLimitId": {
                "codex": {
                    "primary": {"usedPercent": 55, "windowDurationMins": 300, "resetsAt": 4102444800},
                    "secondary": {"usedPercent": 12, "windowDurationMins": 10080, "resetsAt": 4102444800},
                    "planType": "pro"
                }
            }
        }})
    elif method == "turn/start":
        send({"jsonrpc": "2.0", "id": mid, "result": {"turn": {"id": "turn-1"}}})
        send({"jsonrpc": "2.0", "method": "turn/completed", "params": {
            "threadId": "thr-1", "turn": {"id": "turn-1", "status": "completed"}
        }})
"#,
    );

    let prev = std::env::var_os("KVISTCODE_CODEX_COMMAND");
    std::env::set_var(
        "KVISTCODE_CODEX_COMMAND",
        format!("python3 {}", mock.display()),
    );

    let core = Core::open(data, Arc::new(Quiet)).unwrap();
    let acc = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == Provider::Codex)
        .unwrap();

    let agent = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Codex,
            account_id: acc.id.clone(),
            custom_agent_id: None,
            project_path: proj.to_string_lossy().into_owned(),
            worktree: false,
            title: Some("Codex".into()),
            prompt: "hi".into(),
            attachments: Vec::new(),
            group_id: None,
            model: None,
            permission_mode: None,
            config_choices: Default::default(),
        })
        .await
        .unwrap();

    let _ = wait_idle(&core, &agent.id).await;
    // Wait briefly for the spawned `account/rateLimits/read` task to land.
    let mut got = None;
    for _ in 0..80 {
        if let Some(u) = core.account(&acc.id).unwrap().usage {
            got = Some(u);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let u = got.expect("codex rate limits read on connect");
    assert!((u.primary.unwrap().used_percent - 55.0).abs() < 1e-6);
    assert!((u.secondary.unwrap().used_percent - 12.0).abs() < 1e-6);
    assert_eq!(u.plan.as_deref(), Some("pro"));

    core.shutdown().await;
    match prev {
        Some(v) => std::env::set_var("KVISTCODE_CODEX_COMMAND", v),
        None => std::env::remove_var("KVISTCODE_CODEX_COMMAND"),
    }
    let _ = std::fs::remove_dir_all(&root);
}
