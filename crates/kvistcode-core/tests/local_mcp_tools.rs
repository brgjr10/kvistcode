#![cfg(unix)]

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use kvistcode_core::local_mcp::{self, LocalMcpSettings};
use kvistcode_core::{
    AgentStatus, Core, CreateAgentRequest, EventSink, ItemBody, KvistcodeEvent, Provider,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

struct ChanSink(mpsc::UnboundedSender<KvistcodeEvent>);
impl EventSink for ChanSink {
    fn emit(&self, event: KvistcodeEvent) {
        let _ = self.0.send(event);
    }
}

fn init_git_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.name", "Test"]);
    run(&["config", "user.email", "test@example.com"]);
    std::fs::write(dir.join("README.md"), "# Demo\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
}

async fn call_tool(
    core: &Core,
    settings: &LocalMcpSettings,
    name: &str,
    arguments: Value,
) -> (bool, Value) {
    let resp = local_mcp::handle_jsonrpc(
        core,
        settings,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        }),
    )
    .await
    .expect("tools/call returns response");
    let is_err = resp["result"]["isError"].as_bool().unwrap_or(false);
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| Value::String(text));
    (is_err, parsed)
}

#[tokio::test]
async fn fleet_tools_diff_summary_spawn_and_send_message() {
    let data_dir = common::scratch("local-mcp-data");
    let repo_dir = common::scratch("local-mcp-repo");
    init_git_repo(&repo_dir);

    // Point Grok at our mock ACP script so spawn_agent & send_message complete cleanly.
    let mock_acp = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mock_acp.py");
    std::env::set_var(
        "KVISTCODE_GROK_COMMAND",
        format!("python3 {}", mock_acp.display()),
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    let core = Core::open(data_dir.clone(), Arc::new(ChanSink(tx))).unwrap();
    // Auto-approve mock_acp's single permission prompt so turns finish cleanly.
    let approver_core = core.clone();
    let approver = tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let KvistcodeEvent::ItemUpserted { item } = ev {
                if let ItemBody::Permission {
                    request_id,
                    resolution: None,
                    ..
                } = item.body
                {
                    let _ = approver_core
                        .respond_permission(&item.agent_id, &request_id, "yes")
                        .await;
                }
            }
        }
    });
    let read_only = LocalMcpSettings {
        enabled: true,
        port: 47899,
        allow_read_tools: true,
        allow_write_tools: false,
        expose_to_agents: true,
    };
    let read_write = LocalMcpSettings {
        allow_write_tools: true,
        ..read_only.clone()
    };

    // 1. spawn_agent is rejected while allow_write_tools is false.
    let (is_err, msg) = call_tool(
        &core,
        &read_only,
        "spawn_agent",
        json!({
            "provider": "grok",
            "project_path": repo_dir.to_string_lossy(),
            "prompt": "Hello from fleet",
            "worktree": true,
        }),
    )
    .await;
    assert!(is_err, "expected write gate error, got {msg}");

    // 2. With allow_write_tools = true, spawn_agent creates a worktree agent using default account.
    let (is_err, spawned) = call_tool(
        &core,
        &read_write,
        "spawn_agent",
        json!({
            "provider": "grok",
            "project_path": repo_dir.to_string_lossy(),
            "prompt": "Say hello",
            "title": "Fleet Worker",
            "worktree": true,
        }),
    )
    .await;
    assert!(!is_err, "spawn_agent failed: {spawned}");
    let agent_id = spawned["id"].as_str().unwrap().to_string();
    assert_eq!(spawned["title"], "Fleet Worker");
    assert_eq!(spawned["provider"], "grok");
    assert_eq!(spawned["worktree"], true);

    // Wait for first turn to complete so timeline has an assistant message.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let a = core.agent(&agent_id).unwrap();
        if a.status == AgentStatus::Idle && a.last_message.is_some() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "agent did not finish first turn: status={:?}",
            a.status
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 3. list_agents returns the spawned agent.
    let (is_err, listed) = call_tool(&core, &read_only, "list_agents", json!({})).await;
    assert!(!is_err);
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["agents"][0]["id"], agent_id);

    // 4. get_agent_status returns detailed agent metadata.
    let (is_err, status) = call_tool(
        &core,
        &read_only,
        "get_agent_status",
        json!({ "agent_id": agent_id }),
    )
    .await;
    assert!(!is_err);
    assert_eq!(status["id"], agent_id);
    assert_eq!(status["status"], "idle");
    assert_eq!(status["worktree"], true);

    // 5. Modify a file in the agent's worktree with a secret-looking token; get_diff redacts it.
    let agent = core.agent(&agent_id).unwrap();
    let fake_secret = ["s", "k-ant-api03-AbCdEf_ghIJKL-mnopqrstuvwx1234"].concat();
    std::fs::write(
        Path::new(&agent.cwd).join("README.md"),
        format!("# Demo\nAPI_KEY={fake_secret}\n"),
    )
    .unwrap();

    let (is_err, diff) = call_tool(
        &core,
        &read_only,
        "get_diff",
        json!({ "agent_id": agent_id }),
    )
    .await;
    assert!(!is_err, "get_diff failed: {diff}");
    assert_eq!(diff["isGitRepo"], true);
    let patch = diff["patch"].as_str().unwrap();
    assert!(patch.contains("README.md"), "{patch}");
    assert!(
        !patch.contains(&fake_secret),
        "secret leaked in diff: {patch}"
    );
    assert!(patch.contains("[REDACTED]"), "{patch}");

    // 6. read_summary returns latest assistant messages.
    let (is_err, summary) = call_tool(
        &core,
        &read_only,
        "read_summary",
        json!({ "agent_id": agent_id, "max_messages": 3 }),
    )
    .await;
    assert!(!is_err, "read_summary failed: {summary}");
    assert!(!summary["messages"].as_array().unwrap().is_empty());

    // 7. send_message is blocked when allow_write_tools is false, succeeds when true.
    let (is_err, _) = call_tool(
        &core,
        &read_only,
        "send_message",
        json!({ "agent_id": agent_id, "message": "Follow up" }),
    )
    .await;
    assert!(is_err);

    let (is_err, sent) = call_tool(
        &core,
        &read_write,
        "send_message",
        json!({ "agent_id": agent_id, "message": "Follow up" }),
    )
    .await;
    assert!(!is_err, "send_message failed: {sent}");
    assert_eq!(sent["ok"], true);

    // 8. Verify expose_to_agents injects the kvistcode HTTP MCP entry with Bearer header.
    let entry = local_mcp::agent_mcp_server(&read_only, &data_dir).unwrap();
    assert_eq!(entry.name, "kvistcode");
    assert_eq!(entry.url, "http://127.0.0.1:47899/mcp");

    // Create an archived agent and confirm list_agents hides archived by default.
    let acct = core
        .accounts()
        .unwrap()
        .into_iter()
        .find(|a| a.provider == Provider::Grok)
        .unwrap();
    let extra = core
        .create_agent(CreateAgentRequest {
            provider: Provider::Grok,
            account_id: acct.id,
            project_path: repo_dir.to_string_lossy().into_owned(),
            prompt: String::new(),
            title: Some("Archived Agent".into()),
            worktree: false,
            model: None,
            group_id: None,
            custom_agent_id: None,
            permission_mode: None,
            attachments: Vec::new(),
            config_choices: Default::default(),
        })
        .await
        .unwrap();
    core.archive_agent(&extra.id, true).unwrap();
    let (_, listed_default) = call_tool(&core, &read_only, "list_agents", json!({})).await;
    assert_eq!(listed_default["count"], 1);
    let (_, listed_all) = call_tool(
        &core,
        &read_only,
        "list_agents",
        json!({ "include_archived": true }),
    )
    .await;
    assert_eq!(listed_all["count"], 2);

    approver.abort();
    core.shutdown().await;
}
