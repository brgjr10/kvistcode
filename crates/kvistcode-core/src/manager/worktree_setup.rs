//! Orchestrates per-project worktree setup and teardown hooks inside `Core`.

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::model::*;
use crate::worktree_hooks::{
    self, ResolvedWorktreeHooks, WorktreeSetupState, WorktreeSetupStatus, SETUP_TIMEOUT,
    TEARDOWN_TIMEOUT,
};

use super::Core;

/// Deferred initial prompt / warm-up waiting for worktree setup (or repo-hook approval).
pub(super) struct PendingStart {
    pub prompt: Option<(String, Vec<Attachment>)>,
    pub model: Option<String>,
}

impl Core {
    /// Resolves effective worktree hooks (user Settings + detected repo config) for `project_path`.
    pub fn get_worktree_hooks(&self, project_path: &str) -> Result<ResolvedWorktreeHooks> {
        let trimmed = project_path.trim();
        if trimmed.is_empty() {
            bail!("Pick a project folder first");
        }
        let settings = self.inner.settings.lock().clone();
        Ok(worktree_hooks::resolve(trimmed, &settings))
    }

    /// Saves or revokes trust for a project's repo-level worktree commands.
    /// Pass `Some(script_hash)` to trust that exact hash, or `None` to revoke trust.
    pub fn set_worktree_hooks_trust(
        &self,
        project_path: &str,
        script_hash: Option<String>,
    ) -> Result<ResolvedWorktreeHooks> {
        let trimmed = project_path.trim();
        if trimmed.is_empty() {
            bail!("Pick a project folder first");
        }
        if let Some(ref hash) = script_hash {
            if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                bail!("Invalid worktree script hash");
            }
        }
        let key = worktree_hooks::norm_project_key(trimmed);
        self.update_settings(|s| {
            worktree_hooks::map_remove_matching(&mut s.trusted_worktree_hooks, trimmed);
            if let Some(hash) = script_hash.clone() {
                s.trusted_worktree_hooks.insert(key.clone(), hash);
            }
        })?;
        let settings = self.inner.settings.lock().clone();
        Ok(worktree_hooks::resolve(trimmed, &settings))
    }

    /// Runs worktree setup hooks (or pauses for repo-script approval) right after a worktree
    /// agent is created. Returns `true` when setup took ownership of launching `pending`.
    pub(super) fn begin_worktree_setup(&self, agent: &Agent, pending: PendingStart) -> bool {
        if !agent.worktree {
            return false;
        }
        let settings = self.inner.settings.lock().clone();
        let resolved = worktree_hooks::resolve(&agent.project_path, &settings);

        if let Some(err) = resolved
            .repo
            .as_ref()
            .and_then(|r| r.parse_error.as_deref())
        {
            self.ctx(&agent.id).notice(
                NoticeLevel::Warn,
                format!("Ignored repo worktree config: {err}"),
            );
        }

        if !resolved.has_setup_work() {
            return false;
        }

        self.inner
            .pending_starts
            .lock()
            .insert(agent.id.clone(), pending);

        if resolved.setup_from_repo && resolved.needs_trust {
            let repo = resolved.repo.as_ref();
            let source_path = repo.map(|r| r.source_path.clone());
            let state = WorktreeSetupState {
                status: WorktreeSetupStatus::NeedsTrust,
                source_path: source_path.clone(),
                setup_command: resolved.setup_command.clone(),
                teardown_command: if resolved.teardown_from_repo {
                    resolved.teardown_command.clone()
                } else {
                    None
                },
                script_hash: repo.and_then(|r| r.script_hash.clone()),
                error: None,
                output: None,
            };
            let _ = self.update_agent(&agent.id, |a| {
                a.status = AgentStatus::Idle;
                a.worktree_setup = Some(state.clone());
                a.last_activity_at = now_ms();
            });
            let src = source_path.as_deref().unwrap_or("this repository");
            self.ctx(&agent.id).notice(
                NoticeLevel::Warn,
                format!(
                    "Worktree setup is waiting for your approval: {src} defines shell commands. Review and approve them below, or skip setup."
                ),
            );
            return true;
        }

        let state = running_state(&resolved);
        let _ = self.update_agent(&agent.id, |a| {
            a.status = AgentStatus::Starting;
            a.worktree_setup = Some(state.clone());
            a.last_activity_at = now_ms();
        });
        self.spawn_worktree_setup(agent.id.clone(), resolved);
        true
    }

    /// Approves repo-defined worktree hooks for an agent waiting on trust, optionally
    /// remembering the script hash for future worktrees in this project.
    pub async fn approve_worktree_hooks(
        &self,
        agent_id: &str,
        trust_always: bool,
    ) -> Result<Agent> {
        let agent = self.agent(agent_id)?;
        if !agent.worktree {
            bail!("This agent is not running in a git worktree");
        }
        let settings = self.inner.settings.lock().clone();
        let resolved = worktree_hooks::resolve(&agent.project_path, &settings);

        let expected_hash = agent
            .worktree_setup
            .as_ref()
            .and_then(|s| s.script_hash.clone());
        let current_hash = resolved.repo.as_ref().and_then(|r| r.script_hash.clone());

        if let (Some(expected), Some(current)) = (&expected_hash, &current_hash) {
            if expected != current {
                // The repo file changed while the prompt was open: refresh the displayed commands.
                let refreshed = WorktreeSetupState {
                    status: WorktreeSetupStatus::NeedsTrust,
                    source_path: resolved.repo.as_ref().map(|r| r.source_path.clone()),
                    setup_command: resolved.setup_command.clone(),
                    teardown_command: if resolved.teardown_from_repo {
                        resolved.teardown_command.clone()
                    } else {
                        None
                    },
                    script_hash: Some(current.clone()),
                    error: None,
                    output: None,
                };
                let _ = self.update_agent(agent_id, |a| {
                    a.worktree_setup = Some(refreshed.clone());
                    a.last_activity_at = now_ms();
                });
                bail!(
                    "The repository's worktree config changed on disk. Review the updated commands and approve again."
                );
            }
        }

        if trust_always {
            if let Some(hash) = current_hash {
                self.set_worktree_hooks_trust(&agent.project_path, Some(hash))?;
            }
        }

        let state = running_state(&resolved);
        self.update_agent(agent_id, |a| {
            a.status = AgentStatus::Starting;
            a.worktree_setup = Some(state.clone());
            a.last_activity_at = now_ms();
        })?;
        self.spawn_worktree_setup(agent_id.to_string(), resolved);
        self.agent(agent_id)
    }

    /// Retries a failed worktree setup (re-reading current hooks and respecting the trust gate).
    pub async fn retry_worktree_setup(&self, agent_id: &str) -> Result<Agent> {
        let agent = self.agent(agent_id)?;
        if !agent.worktree {
            bail!("This agent is not running in a git worktree");
        }
        let settings = self.inner.settings.lock().clone();
        let resolved = worktree_hooks::resolve(&agent.project_path, &settings);

        if resolved.setup_from_repo && resolved.needs_trust {
            let repo = resolved.repo.as_ref();
            let state = WorktreeSetupState {
                status: WorktreeSetupStatus::NeedsTrust,
                source_path: repo.map(|r| r.source_path.clone()),
                setup_command: resolved.setup_command.clone(),
                teardown_command: if resolved.teardown_from_repo {
                    resolved.teardown_command.clone()
                } else {
                    None
                },
                script_hash: repo.and_then(|r| r.script_hash.clone()),
                error: None,
                output: None,
            };
            self.update_agent(agent_id, |a| {
                a.status = AgentStatus::Idle;
                a.worktree_setup = Some(state.clone());
                a.last_activity_at = now_ms();
            })?;
            return self.agent(agent_id);
        }

        let state = running_state(&resolved);
        self.update_agent(agent_id, |a| {
            a.status = AgentStatus::Starting;
            a.worktree_setup = Some(state.clone());
            a.last_activity_at = now_ms();
        })?;
        self.spawn_worktree_setup(agent_id.to_string(), resolved);
        self.agent(agent_id)
    }

    /// Skips the worktree setup command (still copying any configured safe files) and lets
    /// the agent start or continue normally.
    pub async fn skip_worktree_hooks(&self, agent_id: &str) -> Result<Agent> {
        let agent = self.agent(agent_id)?;
        let settings = self.inner.settings.lock().clone();
        let resolved = worktree_hooks::resolve(&agent.project_path, &settings);
        if agent.worktree && !resolved.copy_files.is_empty() {
            let proj = PathBuf::from(&agent.project_path);
            let wt = PathBuf::from(&agent.cwd);
            let patterns = resolved.copy_files.clone();
            if let Ok(summary) = tokio::task::spawn_blocking(move || {
                worktree_hooks::copy_files(&proj, &wt, &patterns)
            })
            .await
            {
                self.emit_copy_notices(agent_id, &summary);
            }
        }
        let pending = self.inner.pending_starts.lock().remove(agent_id);
        let will_start = pending.is_some();
        self.update_agent(agent_id, |a| {
            a.worktree_setup = None;
            if will_start {
                a.status = AgentStatus::Starting;
            } else if a.status == AgentStatus::Error {
                a.status = AgentStatus::Idle;
            }
            a.last_activity_at = now_ms();
        })?;
        self.ctx(agent_id)
            .notice(NoticeLevel::Info, "Skipped worktree setup command.");
        if let Some(p) = pending {
            self.launch_pending_start(agent_id, p);
        }
        self.agent(agent_id)
    }

    /// Runs the configured teardown command inside a worktree right before it is deleted.
    /// Never runs untrusted repo commands; failures are logged and never block deletion.
    pub(super) async fn run_worktree_teardown(&self, agent: &Agent) {
        if !agent.worktree {
            return;
        }
        let wt = PathBuf::from(&agent.cwd);
        if !wt.is_dir() {
            return;
        }
        let settings = self.inner.settings.lock().clone();
        let resolved = worktree_hooks::resolve(&agent.project_path, &settings);
        let Some(cmd) = resolved.teardown_command.as_deref() else {
            return;
        };
        if resolved.teardown_from_repo && resolved.needs_trust {
            tracing::info!(
                "skipping untrusted repo teardown command for agent {}",
                agent.id
            );
            return;
        }
        let project = PathBuf::from(&agent.project_path);
        let outcome = worktree_hooks::run_hook_command(
            cmd,
            &wt,
            &project,
            agent.branch.as_deref(),
            &agent.id,
            TEARDOWN_TIMEOUT,
        )
        .await;
        if !outcome.success {
            tracing::warn!(
                "worktree teardown command failed for agent {}: {} ({})",
                agent.id,
                outcome.error.as_deref().unwrap_or("unknown error"),
                outcome.output.lines().last().unwrap_or("")
            );
        }
    }

    fn spawn_worktree_setup(&self, agent_id: String, resolved: ResolvedWorktreeHooks) {
        let core = self.clone();
        tokio::spawn(async move {
            let Ok(agent) = core.agent(&agent_id) else {
                return;
            };
            if core.is_shutting_down() {
                return;
            }

            // 1. Copy configured files/globs safely from the main checkout.
            if !resolved.copy_files.is_empty() {
                let proj = PathBuf::from(&agent.project_path);
                let wt = PathBuf::from(&agent.cwd);
                let patterns = resolved.copy_files.clone();
                match tokio::task::spawn_blocking(move || {
                    worktree_hooks::copy_files(&proj, &wt, &patterns)
                })
                .await
                {
                    Ok(summary) => core.emit_copy_notices(&agent_id, &summary),
                    Err(e) => core.ctx(&agent_id).notice(
                        NoticeLevel::Warn,
                        format!("Couldn't copy worktree files: {e}"),
                    ),
                }
            }

            if core.is_shutting_down() || core.agent(&agent_id).is_err() {
                return;
            }

            // 2. Run setup command if configured.
            if let Some(cmd) = resolved.setup_command.as_deref() {
                let proj = PathBuf::from(&agent.project_path);
                let wt = PathBuf::from(&agent.cwd);
                let outcome = worktree_hooks::run_hook_command(
                    cmd,
                    &wt,
                    &proj,
                    agent.branch.as_deref(),
                    &agent_id,
                    SETUP_TIMEOUT,
                )
                .await;

                if core.is_shutting_down() || core.agent(&agent_id).is_err() {
                    return;
                }

                if !outcome.success {
                    let err_msg = outcome
                        .error
                        .clone()
                        .unwrap_or_else(|| "Setup command failed".into());
                    let output = (!outcome.output.is_empty()).then_some(outcome.output.clone());
                    let failed = WorktreeSetupState {
                        status: WorktreeSetupStatus::Failed,
                        source_path: if resolved.setup_from_repo {
                            resolved.repo.as_ref().map(|r| r.source_path.clone())
                        } else {
                            None
                        },
                        setup_command: Some(cmd.to_string()),
                        teardown_command: None,
                        script_hash: resolved.repo.as_ref().and_then(|r| r.script_hash.clone()),
                        error: Some(err_msg.clone()),
                        output: output.clone(),
                    };
                    let _ = core.update_agent(&agent_id, |a| {
                        a.status = AgentStatus::Error;
                        a.worktree_setup = Some(failed.clone());
                        a.last_activity_at = now_ms();
                    });
                    let mut notice = format!("Worktree setup failed ({err_msg}): `{cmd}`");
                    if let Some(out) = output {
                        notice.push_str("\n\n");
                        notice.push_str(&out);
                    }
                    core.ctx(&agent_id).notice(NoticeLevel::Error, notice);
                    return;
                }

                core.ctx(&agent_id)
                    .notice(NoticeLevel::Info, "Worktree setup finished.");
            }

            // 3. Setup succeeded: clear setup state and launch deferred prompt / warm-up.
            let pending = core.inner.pending_starts.lock().remove(&agent_id);
            let will_start = pending.is_some();
            let _ = core.update_agent(&agent_id, |a| {
                a.worktree_setup = None;
                if will_start {
                    a.status = AgentStatus::Starting;
                } else if a.status == AgentStatus::Starting || a.status == AgentStatus::Error {
                    a.status = AgentStatus::Idle;
                }
                a.last_activity_at = now_ms();
            });
            if let Some(p) = pending {
                core.launch_pending_start(&agent_id, p);
            }
        });
    }

    fn emit_copy_notices(&self, agent_id: &str, summary: &worktree_hooks::CopySummary) {
        if !summary.copied.is_empty() {
            let listed = if summary.copied.len() <= 5 {
                summary.copied.join(", ")
            } else {
                format!(
                    "{}, and {} more",
                    summary.copied[..5].join(", "),
                    summary.copied.len() - 5
                )
            };
            let noun = if summary.copied.len() == 1 {
                "file"
            } else {
                "files"
            };
            self.ctx(agent_id).notice(
                NoticeLevel::Info,
                format!(
                    "Copied {} {noun} into worktree: {listed}",
                    summary.copied.len()
                ),
            );
        }
        if !summary.warnings.is_empty() {
            self.ctx(agent_id).notice(
                NoticeLevel::Warn,
                format!(
                    "Worktree file copy warnings:\n{}",
                    summary.warnings.join("\n")
                ),
            );
        }
    }

    fn launch_pending_start(&self, agent_id: &str, pending: PendingStart) {
        let ctx = self.ctx(agent_id);
        match pending.prompt {
            None => self.warm_up(agent_id, pending.model),
            Some((text, attachments)) => {
                if let Some(p) = ctx.submit_prompt(&text, attachments) {
                    self.run_prompt(agent_id, p, pending.model);
                }
            }
        }
    }
}

fn running_state(resolved: &ResolvedWorktreeHooks) -> WorktreeSetupState {
    WorktreeSetupState {
        status: WorktreeSetupStatus::Running,
        source_path: if resolved.setup_from_repo {
            resolved.repo.as_ref().map(|r| r.source_path.clone())
        } else {
            None
        },
        setup_command: resolved.setup_command.clone(),
        teardown_command: None,
        script_hash: resolved.repo.as_ref().and_then(|r| r.script_hash.clone()),
        error: None,
        output: None,
    }
}
