//! Non-interrupting side-chat (`/btw <question>`): answers a quick read-only
//! question against the agent's workspace without stopping, steering, or queueing
//! behind the main agent's active turn.
//!
//! Runs an ephemeral, strictly read-only session (ACP or Codex) in the agent's
//! `cwd`, streams updates via `KvistcodeEvent::SideChatUpdated`, and kills the
//! temporary process as soon as the answer finishes or times out.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::*;
use crate::jsonrpc::{Incoming, Peer, SpawnOptions};

const SIDE_CHAT_TIMEOUT: Duration = Duration::from_secs(120);
const STREAM_EMIT_INTERVAL: Duration = Duration::from_millis(60);
const ACP_PROTOCOL_VERSION: i64 = 1;

const READ_ONLY_INSTRUCTIONS: &str = "\
You are answering a quick, non-interrupting side question (/btw) from the user while \
the main coding agent continues working in this workspace. \
STRICT RULES: \
1. READ-ONLY ONLY: Do NOT edit, create, or delete files, do NOT run mutating commands, \
and do NOT change git state. \
2. Answer concisely and directly in plain markdown.";

impl Core {
    /// Runs a quick read-only side question against `agent_id`'s workspace without
    /// interrupting or queueing behind any running turn. Returns the initial
    /// `Running` entry immediately; progress and completion stream via
    /// `KvistcodeEvent::SideChatUpdated`.
    pub async fn ask_side_question(&self, agent_id: &str, question: &str) -> Result<SideChatEntry> {
        let question = question.trim();
        if question.is_empty() {
            bail!("Ask a question after /btw");
        }
        if self.is_shutting_down() {
            bail!(SHUTTING_DOWN);
        }
        let agent = self.agent(agent_id)?;
        let account = self.account(&agent.account_id)?;
        let cwd = PathBuf::from(&agent.cwd);
        if !cwd.is_dir() {
            bail!("The agent's folder no longer exists: {}", agent.cwd);
        }

        let entry = SideChatEntry {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent.id.clone(),
            question: question.to_string(),
            answer: String::new(),
            status: SideChatStatus::Running,
            error: None,
            created_at: now_ms(),
            finished_at: None,
        };
        self.emit(KvistcodeEvent::SideChatUpdated {
            entry: entry.clone(),
        });

        let context = self.transcript(&agent.id, 3_000);
        let core = self.clone();
        let mut live_entry = entry.clone();
        tokio::spawn(async move {
            let outcome = tokio::time::timeout(
                SIDE_CHAT_TIMEOUT,
                core.run_side_question(&agent, &account, &cwd, &context, &mut live_entry),
            )
            .await;
            match outcome {
                Ok(Ok(())) => {
                    live_entry.status = SideChatStatus::Done;
                    live_entry.finished_at = Some(now_ms());
                }
                Ok(Err(e)) => {
                    live_entry.status = SideChatStatus::Error;
                    live_entry.error = Some(format!("{e:#}"));
                    live_entry.finished_at = Some(now_ms());
                }
                Err(_) => {
                    live_entry.status = SideChatStatus::Error;
                    live_entry.error = Some("Side question timed out".to_string());
                    live_entry.finished_at = Some(now_ms());
                }
            }
            core.emit(KvistcodeEvent::SideChatUpdated { entry: live_entry });
        });

        Ok(entry)
    }

    async fn run_side_question(
        &self,
        agent: &Agent,
        account: &Account,
        cwd: &Path,
        context: &str,
        entry: &mut SideChatEntry,
    ) -> Result<()> {
        let (program, args, env, env_remove) = if agent.provider == Provider::Custom {
            let def = self.custom_agent_def(agent.custom_agent_id.as_deref())?;
            let (program, args) = custom_launch(&def)?;
            let env = def
                .env
                .iter()
                .map(|e| (e.name.clone(), e.value.clone()))
                .collect();
            (program, args, env, Vec::new())
        } else {
            let (program, mut args) = providers::launch_spec_for_account(
                agent.provider,
                Some(account),
                &self.inner.data_dir,
            )?;
            let acct = account.clone();
            let (has_key, acct_env, acct_env_remove) = tokio::task::spawn_blocking(move || {
                (
                    accounts::get_api_key(&acct.id).is_some(),
                    accounts::account_env(&acct),
                    accounts::env_to_remove(&acct),
                )
            })
            .await
            .map_err(|e| anyhow!("reading the account's credentials: {e}"))?;
            if agent.provider == Provider::Local {
                let server = account
                    .local
                    .as_ref()
                    .ok_or_else(|| anyhow!("This account has no model server configured"))?;
                args.extend(local::codex_override_args(
                    server,
                    agent.model.as_deref(),
                    has_key,
                ));
            }
            if agent.provider == Provider::Gemini {
                crate::gemini::prepare_home(account);
            }
            (program, args, acct_env, acct_env_remove)
        };

        let prompt = build_side_prompt(context, &entry.question);
        let label = format!("btw:{}", &entry.id[..8.min(entry.id.len())]);
        let (peer, rx) = Peer::spawn(SpawnOptions {
            program: &program,
            args: &args,
            cwd,
            env: &env,
            env_remove: &env_remove,
            label: &label,
            children_file: None,
        })?;

        let result = match agent.provider {
            Provider::Codex | Provider::Local => {
                self.run_codex_side(peer.clone(), rx, cwd, &prompt, entry)
                    .await
            }
            _ => {
                self.run_acp_side(peer.clone(), rx, cwd, &prompt, entry)
                    .await
            }
        };
        peer.kill().await;
        result
    }

    async fn run_acp_side(
        &self,
        peer: Arc<Peer>,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Incoming>,
        cwd: &Path,
        prompt: &str,
        entry: &mut SideChatEntry,
    ) -> Result<()> {
        let init = peer
            .request_timeout(
                "initialize",
                json!({
                    "protocolVersion": ACP_PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": { "readTextFile": false, "writeTextFile": false },
                        "terminal": false
                    },
                    "clientInfo": {
                        "name": "kvistcode",
                        "title": "Kvistcode",
                        "version": VERSION
                    }
                }),
                Some(Duration::from_secs(30)),
            )
            .await?;

        let mut new_params = json!({
            "cwd": cwd.to_string_lossy(),
            "mcpServers": [],
        });
        if init["agentCapabilities"]["sessionCapabilities"]
            .get("systemPrompt")
            .is_some()
        {
            new_params["systemPrompt"] = json!({
                "mode": "append",
                "content": [{ "type": "text", "text": READ_ONLY_INSTRUCTIONS }],
            });
        }
        let created = peer
            .request_timeout("session/new", new_params, Some(Duration::from_secs(30)))
            .await?;
        let session_id = created["sessionId"]
            .as_str()
            .ok_or_else(|| anyhow!("ACP side-chat returned no sessionId"))?
            .to_string();

        let prompt_fut = peer
            .send_request(
                "session/prompt",
                json!({
                    "sessionId": session_id,
                    "prompt": [{ "type": "text", "text": prompt }],
                }),
            )
            .await?;
        tokio::pin!(prompt_fut);

        let mut last_emit = Instant::now();
        let mut dirty = false;
        loop {
            tokio::select! {
                res = &mut prompt_fut => {
                    match res {
                        Ok(_) => {
                            // Drain every notification already read before the response arrived.
                            peer.barrier(1);
                            while let Some(msg) = rx.recv().await {
                                match msg {
                                    Incoming::Barrier(1) | Incoming::Closed => break,
                                    Incoming::Notification { method, params }
                                        if method == "session/update" =>
                                    {
                                        if let Some(chunk) = acp_text_chunk(&params) {
                                            entry.answer.push_str(chunk);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            return Ok(());
                        }
                        Err(e) => bail!("side-chat failed: {e}"),
                    }
                }
                incoming = rx.recv() => {
                    let Some(msg) = incoming else {
                        bail!("side-chat agent closed unexpectedly");
                    };
                    match msg {
                        Incoming::Notification { method, params } if method == "session/update" => {
                            if let Some(chunk) = acp_text_chunk(&params) {
                                entry.answer.push_str(chunk);
                                dirty = true;
                                if last_emit.elapsed() >= STREAM_EMIT_INTERVAL {
                                    self.emit(KvistcodeEvent::SideChatUpdated { entry: entry.clone() });
                                    last_emit = Instant::now();
                                    dirty = false;
                                }
                            }
                        }
                        Incoming::Request { id, method, params } if method == "session/request_permission" => {
                            // Enforce read-only: allow only read/search/think tools; reject writes/commands.
                            let outcome = read_only_permission_outcome(&params);
                            let _ = peer.respond(id, json!({ "outcome": outcome })).await;
                        }
                        Incoming::Request { id, .. } => {
                            let _ = peer.respond_error(id, -32601, "unsupported in side-chat").await;
                        }
                        Incoming::Closed => {
                            if !entry.answer.trim().is_empty() {
                                return Ok(());
                            }
                            bail!("side-chat agent exited before answering");
                        }
                        _ => {}
                    }
                    if dirty && last_emit.elapsed() >= STREAM_EMIT_INTERVAL {
                        self.emit(KvistcodeEvent::SideChatUpdated { entry: entry.clone() });
                        last_emit = Instant::now();
                        dirty = false;
                    }
                }
            }
        }
    }

    async fn run_codex_side(
        &self,
        peer: Arc<Peer>,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Incoming>,
        cwd: &Path,
        prompt: &str,
        entry: &mut SideChatEntry,
    ) -> Result<()> {
        peer.request_timeout(
            "initialize",
            json!({ "clientInfo": { "name": "kvistcode", "title": "Kvistcode", "version": VERSION } }),
            Some(Duration::from_secs(30)),
        )
        .await?;
        peer.notify("initialized", json!({})).await?;

        let resp = peer
            .request_timeout(
                "thread/start",
                json!({
                    "cwd": cwd.to_string_lossy(),
                    "approvalPolicy": "never",
                    "sandbox": "read-only",
                    "serviceName": "kvistcode-btw",
                    "developerInstructions": READ_ONLY_INSTRUCTIONS,
                }),
                Some(Duration::from_secs(30)),
            )
            .await?;
        let thread_id = resp["thread"]["id"]
            .as_str()
            .ok_or_else(|| anyhow!("Codex side-chat returned no thread id"))?
            .to_string();

        peer.request_timeout(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt }],
            }),
            Some(Duration::from_secs(30)),
        )
        .await?;

        let mut last_emit = Instant::now();
        while let Some(msg) = rx.recv().await {
            match msg {
                Incoming::Notification { method, params } => match method.as_str() {
                    "item/agentMessage/delta" => {
                        if let Some(delta) = params["delta"].as_str() {
                            entry.answer.push_str(delta);
                            if last_emit.elapsed() >= STREAM_EMIT_INTERVAL {
                                self.emit(KvistcodeEvent::SideChatUpdated {
                                    entry: entry.clone(),
                                });
                                last_emit = Instant::now();
                            }
                        }
                    }
                    "item/completed" => {
                        let item = &params["item"];
                        if item["type"].as_str() == Some("agentMessage") && entry.answer.is_empty()
                        {
                            if let Some(text) = item["text"].as_str() {
                                entry.answer.push_str(text);
                            }
                        }
                    }
                    "turn/completed" => {
                        let status = params["turn"]["status"].as_str().unwrap_or("completed");
                        if status == "failed" {
                            let msg = params["turn"]["error"]["message"]
                                .as_str()
                                .unwrap_or("side-chat turn failed");
                            bail!("{msg}");
                        }
                        return Ok(());
                    }
                    _ => {}
                },
                Incoming::Request { id, .. } => {
                    // Read-only side chat never approves interactive write/command requests.
                    let _ = peer.respond(id, json!({ "decision": "decline" })).await;
                }
                Incoming::Closed => {
                    if !entry.answer.trim().is_empty() {
                        return Ok(());
                    }
                    bail!("side-chat agent exited before answering");
                }
                Incoming::Barrier(_) => {}
            }
        }
        Ok(())
    }
}

fn build_side_prompt(context: &str, question: &str) -> String {
    let mut out = String::from(READ_ONLY_INSTRUCTIONS);
    out.push_str("\n\n");
    if !context.trim().is_empty() {
        out.push_str("Recent context from the main agent's conversation (for reference):\n");
        out.push_str(context);
        out.push_str("\n\n");
    }
    out.push_str("Side question (/btw): ");
    out.push_str(question);
    out
}

fn acp_text_chunk(params: &Value) -> Option<&str> {
    let upd = &params["update"];
    if upd["sessionUpdate"].as_str() != Some("agent_message_chunk") {
        return None;
    }
    // Skip sub-agent chatter if attributed to a parent tool call.
    if upd["_meta"]["claudeCode"]["parentToolUseId"].is_string() {
        return None;
    }
    upd["content"]["text"].as_str()
}

fn read_only_permission_outcome(params: &Value) -> Value {
    let kind = params["toolCall"]["kind"].as_str().unwrap_or("");
    let is_read_only = matches!(kind, "read" | "search" | "think");
    let options = params["options"].as_array();
    if is_read_only {
        if let Some(id) = options.and_then(|opts| {
            opts.iter()
                .find(|o| o["kind"].as_str() == Some("allow_once"))
                .and_then(|o| o["optionId"].as_str())
        }) {
            return json!({ "outcome": "selected", "optionId": id });
        }
    }
    if let Some(id) = options.and_then(|opts| {
        opts.iter()
            .find(|o| matches!(o["kind"].as_str(), Some("reject_once" | "reject_always")))
            .and_then(|o| o["optionId"].as_str())
    }) {
        return json!({ "outcome": "selected", "optionId": id });
    }
    json!({ "outcome": "cancelled" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_permission_allows_reads_and_denies_writes() {
        let opts = json!([
            { "optionId": "allow", "kind": "allow_once" },
            { "optionId": "deny", "kind": "reject_once" }
        ]);
        let read = json!({ "toolCall": { "kind": "read" }, "options": opts });
        assert_eq!(
            read_only_permission_outcome(&read),
            json!({ "outcome": "selected", "optionId": "allow" })
        );
        let edit = json!({ "toolCall": { "kind": "edit" }, "options": opts });
        assert_eq!(
            read_only_permission_outcome(&edit),
            json!({ "outcome": "selected", "optionId": "deny" })
        );
        let exec = json!({ "toolCall": { "kind": "execute" }, "options": opts });
        assert_eq!(
            read_only_permission_outcome(&exec),
            json!({ "outcome": "selected", "optionId": "deny" })
        );
    }

    #[test]
    fn acp_text_chunk_ignores_subagent_and_non_message_updates() {
        let ok = json!({
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "hello" }
            }
        });
        assert_eq!(acp_text_chunk(&ok), Some("hello"));
        let sub = json!({
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "_meta": { "claudeCode": { "parentToolUseId": "t1" } },
                "content": { "type": "text", "text": "sub" }
            }
        });
        assert_eq!(acp_text_chunk(&sub), None);
    }
}
