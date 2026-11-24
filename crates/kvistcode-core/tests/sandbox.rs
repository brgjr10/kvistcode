//! Integration tests for per-project OS / container sandboxing settings, persistence,
//! project relocation, and backward compatibility.

mod common;

use std::sync::Arc;

use kvistcode_core::sandbox::{
    detect_sandbox_backends, ActiveSandbox, ProjectSandboxConfig, SandboxBackend, SandboxMode,
};
use kvistcode_core::{Agent, Core, EventSink, KvistcodeEvent, Settings};

struct NoopSink;
impl EventSink for NoopSink {
    fn emit(&self, _event: KvistcodeEvent) {}
}

#[tokio::test]
async fn project_sandboxes_persist_validate_and_relocate() {
    let dir = common::scratch("sandbox-persist");
    let old_proj = dir.join("proj-old");
    let new_proj = dir.join("proj-new");
    std::fs::create_dir_all(&old_proj).unwrap();
    std::fs::create_dir_all(&new_proj).unwrap();
    let old_key = old_proj.to_string_lossy().into_owned();
    let new_key = new_proj.to_string_lossy().into_owned();

    let cfg = ProjectSandboxConfig {
        mode: SandboxMode::Docker,
        allow_network: true,
        docker_image: Some("node:22-bookworm".into()),
        extra_read_paths: vec![old_key.clone()],
    };

    {
        let core = Core::open(dir.clone(), Arc::new(NoopSink)).unwrap();
        core.update_settings(|s| {
            s.project_sandboxes.insert(old_key.clone(), cfg.clone());
        })
        .unwrap();

        let saved = core.settings().unwrap();
        assert_eq!(saved.project_sandboxes.get(&old_key), Some(&cfg));

        // Invalid sandbox config is rejected by Settings::validate.
        let err = core.update_settings(|s| {
            s.project_sandboxes.insert(
                old_key.clone(),
                ProjectSandboxConfig {
                    mode: SandboxMode::Docker,
                    allow_network: false,
                    docker_image: Some("--privileged".into()),
                    extra_read_paths: vec![],
                },
            );
        });
        assert!(err.is_err(), "flag-shaped docker image must be rejected");
    }

    // Re-open from SQLite: config survived restart.
    let core = Core::open(dir.clone(), Arc::new(NoopSink)).unwrap();
    assert_eq!(
        core.settings().unwrap().project_sandboxes.get(&old_key),
        Some(&cfg)
    );

    // Relocating the project moves its sandbox config to the new folder.
    core.relocate_project(&old_key, &new_key).await.unwrap();
    let moved = core.settings().unwrap();
    assert!(!moved.project_sandboxes.contains_key(&old_key));
    assert_eq!(moved.project_sandboxes.get(&new_key), Some(&cfg));
}

#[test]
fn old_settings_and_agent_json_load_cleanly_without_sandbox_fields() {
    let old_settings: Settings = serde_json::from_str(r#"{"theme":"gunmetal"}"#).unwrap();
    assert!(old_settings.project_sandboxes.is_empty());
    assert!(old_settings.validate().is_ok());

    let mut old_agent: Agent = serde_json::from_value(serde_json::json!({
        "id": "a1", "title": "t", "provider": "claude", "accountId": "x",
        "projectPath": "/p", "cwd": "/p", "worktree": false, "branch": null,
        "baseBranch": null, "baseCommit": null, "sessionId": null,
        "status": "idle", "running": false, "groupId": null, "createdAt": 0,
        "updatedAt": 0, "lastActivityAt": 0, "costUsd": 0.0, "tokensUsed": 0,
        "contextUsed": null, "contextSize": null, "unread": false, "archived": false,
        "lastMessage": null, "pendingPermissions": 0
    }))
    .unwrap();
    assert_eq!(old_agent.sandbox_active, None);

    old_agent.sandbox_active = Some(ActiveSandbox {
        backend: SandboxBackend::Bwrap,
        network: false,
    });
    let json = serde_json::to_value(&old_agent).unwrap();
    assert_eq!(
        json["sandboxActive"],
        serde_json::json!({ "backend": "bwrap", "network": false })
    );
}

#[test]
fn detect_sandbox_backends_reports_consistent_capabilities() {
    let caps = detect_sandbox_backends();
    assert_eq!(caps.bwrap_available, caps.bwrap_path.is_some());
    assert_eq!(caps.docker_available, caps.docker_path.is_some());
    assert_eq!(caps.platform, std::env::consts::OS);
    if !cfg!(target_os = "linux") {
        assert!(!caps.bwrap_available);
        assert!(caps.bwrap_path.is_none());
    }
}
