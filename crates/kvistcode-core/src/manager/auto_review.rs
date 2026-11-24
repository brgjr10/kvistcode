//! Orchestrates automatic cross-provider code review inside `Core`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};

use crate::auto_review::{
    self, AutoReviewConfig, AutoReviewFinding, AutoReviewSeverity, AutoReviewState,
    AutoReviewStatus, DiffSnapshot, REVIEWER_SYSTEM_PROMPT,
};
use crate::model::*;
use crate::{accounts, acp, codex, local, providers};

use super::{custom_launch, Core, SHUTTING_DOWN};

/// Timeout for a single cross-provider review pass.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(180);

/// RAII claim ensuring at most one review runs at a time per agent.
struct ReviewClaim {
    core: Core,
    agent_id: String,
}

impl Drop for ReviewClaim {
    fn drop(&mut self) {
        self.core
            .inner
            .running_reviews
            .lock()
            .remove(&self.agent_id);
    }
}

/// Removes the temporary read-only scratch dir created for the reviewer process.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Resolved reviewer target (provider + account/custom def + optional model).
struct ResolvedReviewer {
    provider: Provider,
    account: Option<Account>,
    custom_def: Option<CustomAgent>,
    model: Option<String>,
    label: String,
}

impl Core {
    /// Sets or clears a per-agent auto-review override.
    pub fn set_agent_auto_review(
        &self,
        agent_id: &str,
        mut config: Option<AutoReviewConfig>,
    ) -> Result<Agent> {
        if let Some(ref mut cfg) = config {
            if let Some(problem) = cfg.problem() {
                bail!("{problem}");
            }
            cfg.sanitize();
        }
        self.update_agent(agent_id, |a| {
            a.auto_review = config;
        })
    }

    /// Manually triggers (or re-runs) a cross-provider review for `agent_id`.
    /// Returns immediately with `auto_review_state.status = Running` (or `Passed` if
    /// there are no code changes) and emits `AgentUpdated` when the review finishes.
    pub async fn run_auto_review(&self, agent_id: &str) -> Result<Agent> {
        if self.is_shutting_down() {
            bail!(SHUTTING_DOWN);
        }
        let agent = self.agent(agent_id)?;
        let settings = self.inner.settings.lock().clone();
        let cfg = auto_review::effective_config(&agent, &settings);
        let reviewer = self.resolve_reviewer(&agent, &cfg, &settings)?;

        let Some(claim) = self.claim_review(agent_id) else {
            bail!("A review is already running for this agent");
        };

        let snapshot = match auto_review::collect_agent_diff(&agent).await {
            Ok(Some(snap)) => snap,
            Ok(None) => {
                drop(claim);
                let prev_fix_sent = self.inner.auto_fix_used.lock().contains(agent_id);
                let passed = AutoReviewState {
                    status: AutoReviewStatus::Passed,
                    reviewer_provider: reviewer.provider,
                    reviewer_model: reviewer.model.clone(),
                    summary: "No code changes to review.".into(),
                    findings: Vec::new(),
                    checked_at: now_ms(),
                    auto_fix_sent: prev_fix_sent,
                    error: None,
                    diff_fingerprint: None,
                };
                return self.update_agent(agent_id, |a| {
                    a.auto_review_state = Some(passed);
                });
            }
            Err(e) => {
                drop(claim);
                return Err(e);
            }
        };

        let prev_fix_sent = self.inner.auto_fix_used.lock().contains(agent_id);
        let running_state = AutoReviewState {
            status: AutoReviewStatus::Running,
            reviewer_provider: reviewer.provider,
            reviewer_model: reviewer.model.clone(),
            summary: format!("Reviewing changes with {}…", reviewer.label),
            findings: Vec::new(),
            checked_at: now_ms(),
            auto_fix_sent: prev_fix_sent,
            error: None,
            diff_fingerprint: Some(snapshot.fingerprint.clone()),
        };
        let updated = self.update_agent(agent_id, |a| {
            a.auto_review_state = Some(running_state);
        })?;

        let core = self.clone();
        let id = agent_id.to_string();
        tokio::spawn(async move {
            core.execute_review_pass(id, cfg, reviewer, snapshot, true, claim)
                .await;
        });

        Ok(updated)
    }

    /// Dismisses a single finding from `agent.auto_review_state`.
    pub fn dismiss_auto_review_finding(&self, agent_id: &str, finding_id: &str) -> Result<Agent> {
        let agent = self.agent(agent_id)?;
        let Some(mut state) = agent.auto_review_state.clone() else {
            return Ok(agent);
        };
        let before = state.findings.len();
        state.findings.retain(|f| f.id != finding_id);
        if state.findings.len() == before {
            return Ok(agent);
        }
        if state.status == AutoReviewStatus::Findings && state.findings.is_empty() {
            state.status = AutoReviewStatus::Passed;
            state.summary = "All findings dismissed.".into();
        }
        self.update_agent(agent_id, |a| {
            a.auto_review_state = Some(state);
        })
    }

    /// Sends selected findings (or all findings if `finding_ids` is empty) back to the main
    /// agent to fix, removing the sent findings from the pending review list.
    pub async fn send_auto_review_findings(
        &self,
        agent_id: &str,
        finding_ids: Vec<String>,
    ) -> Result<Agent> {
        if self.is_shutting_down() {
            bail!(SHUTTING_DOWN);
        }
        let agent = self.agent(agent_id)?;
        let Some(mut state) = agent.auto_review_state.clone() else {
            bail!("This agent has no review findings to send");
        };
        let chosen: Vec<AutoReviewFinding> = if finding_ids.is_empty() {
            state.findings.clone()
        } else {
            state
                .findings
                .iter()
                .filter(|f| finding_ids.iter().any(|id| id == &f.id))
                .cloned()
                .collect()
        };
        if chosen.is_empty() {
            bail!("No matching review findings to send");
        }
        let reviewer_label = match state.reviewer_model.as_deref() {
            Some(m) if !m.trim().is_empty() => {
                format!("{} · {m}", state.reviewer_provider.display_name())
            }
            _ => state.reviewer_provider.display_name().to_string(),
        };
        let prompt = auto_review::format_findings_prompt(&chosen, &reviewer_label, false);
        self.send_prompt(agent_id, &prompt, Vec::new()).await?;

        let sent_ids: std::collections::HashSet<&str> =
            chosen.iter().map(|f| f.id.as_str()).collect();
        state.findings.retain(|f| !sent_ids.contains(f.id.as_str()));
        if state.status == AutoReviewStatus::Findings && state.findings.is_empty() {
            state.status = AutoReviewStatus::Passed;
            state.summary = "Findings sent to the agent.".into();
        }
        self.update_agent(agent_id, |a| {
            a.auto_review_state = Some(state);
        })
    }

    /// Called after a non-held turn completes cleanly. Runs an automatic cross-provider
    /// review pass when enabled for this agent/project, when actual code changes exist,
    /// and when this exact diff fingerprint has not already been reviewed.
    pub(crate) async fn maybe_run_auto_review(&self, agent_id: &str) {
        if self.is_shutting_down() {
            return;
        }
        let Ok(agent) = self.agent(agent_id) else {
            return;
        };
        // Never auto-review helper agents or archived agents.
        if agent.parent_id.is_some() || agent.archived {
            return;
        }
        // Respect spending/turn caps before spawning an extra review pass.
        if agent.budget_hold.is_some() || self.budget_has_hit(agent_id) {
            return;
        }
        let settings = self.inner.settings.lock().clone();
        let cfg = auto_review::effective_config(&agent, &settings);
        if !cfg.enabled {
            return;
        }

        let Some(claim) = self.claim_review(agent_id) else {
            return;
        };

        let snapshot = match auto_review::collect_agent_diff(&agent).await {
            Ok(Some(snap)) => snap,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("auto-review diff check for {agent_id} failed: {e:#}");
                return;
            }
        };

        // Skip if we already reviewed this exact diff fingerprint (and it didn't fail).
        if let Some(prev) = agent.auto_review_state.as_ref() {
            if prev.status != AutoReviewStatus::Failed
                && prev.diff_fingerprint.as_deref() == Some(snapshot.fingerprint.as_str())
            {
                return;
            }
        }

        let reviewer = match self.resolve_reviewer(&agent, &cfg, &settings) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("{e:#}");
                let fallback_provider = cfg.provider.unwrap_or(agent.provider);
                let failed = AutoReviewState {
                    status: AutoReviewStatus::Failed,
                    reviewer_provider: fallback_provider,
                    reviewer_model: cfg.model.clone(),
                    summary: "Auto-review couldn't start.".into(),
                    findings: Vec::new(),
                    checked_at: now_ms(),
                    auto_fix_sent: self.inner.auto_fix_used.lock().contains(agent_id),
                    error: Some(err_msg.clone()),
                    diff_fingerprint: Some(snapshot.fingerprint),
                };
                let _ = self.update_agent(agent_id, |a| {
                    a.auto_review_state = Some(failed);
                });
                self.ctx(agent_id)
                    .notice(NoticeLevel::Warn, format!("Auto-review skipped: {err_msg}"));
                return;
            }
        };

        let prev_fix_sent = self.inner.auto_fix_used.lock().contains(agent_id);
        let running = AutoReviewState {
            status: AutoReviewStatus::Running,
            reviewer_provider: reviewer.provider,
            reviewer_model: reviewer.model.clone(),
            summary: format!("Reviewing changes with {}…", reviewer.label),
            findings: Vec::new(),
            checked_at: now_ms(),
            auto_fix_sent: prev_fix_sent,
            error: None,
            diff_fingerprint: Some(snapshot.fingerprint.clone()),
        };
        if self
            .update_agent(agent_id, |a| {
                a.auto_review_state = Some(running);
            })
            .is_err()
        {
            return;
        }

        self.execute_review_pass(agent_id.to_string(), cfg, reviewer, snapshot, false, claim)
            .await;
    }

    async fn execute_review_pass(
        &self,
        agent_id: String,
        cfg: AutoReviewConfig,
        reviewer: ResolvedReviewer,
        snapshot: DiffSnapshot,
        manual: bool,
        _claim: ReviewClaim,
    ) {
        let Ok(agent) = self.agent(&agent_id) else {
            return;
        };
        if self.is_shutting_down() {
            return;
        }

        let prompt = auto_review::build_review_prompt(&agent, &snapshot);
        let outcome = self.invoke_reviewer(&reviewer, &prompt).await;

        if self.is_shutting_down() || self.agent(&agent_id).is_err() {
            return;
        }

        let prev_fix_sent = self.inner.auto_fix_used.lock().contains(&agent_id);

        match outcome {
            Ok(raw_reply) => {
                let parsed = auto_review::parse_review_response(&raw_reply);
                let has_findings = !parsed.findings.is_empty();
                let critical: Vec<AutoReviewFinding> = parsed
                    .findings
                    .iter()
                    .filter(|f| f.severity == AutoReviewSeverity::Critical)
                    .cloned()
                    .collect();

                let should_auto_fix = !manual
                    && cfg.auto_fix_critical
                    && !critical.is_empty()
                    && !prev_fix_sent
                    && !self.is_shutting_down()
                    && !self.ctx(&agent_id).is_turn_active()
                    && self.agent(&agent_id).is_ok_and(|a| {
                        !a.archived && a.budget_hold.is_none() && !a.status.is_busy()
                    })
                    && !self.budget_has_hit(&agent_id)
                    && self.budget_gate(&agent_id, false).is_none();

                let mut auto_fix_sent = prev_fix_sent;
                if should_auto_fix {
                    self.inner.auto_fix_used.lock().insert(agent_id.clone());
                    auto_fix_sent = true;
                }

                let state = AutoReviewState {
                    status: if has_findings {
                        AutoReviewStatus::Findings
                    } else {
                        AutoReviewStatus::Passed
                    },
                    reviewer_provider: reviewer.provider,
                    reviewer_model: reviewer.model.clone(),
                    summary: parsed.summary.clone(),
                    findings: parsed.findings.clone(),
                    checked_at: now_ms(),
                    auto_fix_sent,
                    error: None,
                    diff_fingerprint: Some(snapshot.fingerprint),
                };
                if self
                    .update_agent(&agent_id, |a| {
                        a.auto_review_state = Some(state);
                    })
                    .is_err()
                {
                    return;
                }

                // Emit a concise timeline notice summarizing the review.
                let notice_level = if !critical.is_empty() {
                    NoticeLevel::Warn
                } else {
                    NoticeLevel::Info
                };
                let notice_text = if has_findings {
                    format!(
                        "Cross-provider review ({}) found {}: {}",
                        reviewer.label,
                        super::plural(parsed.findings.len(), "issue", "issues"),
                        parsed.summary
                    )
                } else {
                    format!(
                        "Cross-provider review ({}) passed: {}",
                        reviewer.label, parsed.summary
                    )
                };
                self.ctx(&agent_id).notice(notice_level, notice_text);

                if should_auto_fix {
                    let fix_prompt =
                        auto_review::format_findings_prompt(&critical, &reviewer.label, true);
                    let ctx = self.ctx(&agent_id);
                    if let Some(p) = ctx.submit_automatic_prompt(&fix_prompt) {
                        let running = self.live_driver(&agent_id).is_some();
                        let _ = self.update_agent(&agent_id, |a| {
                            a.unread = false;
                            a.last_activity_at = now_ms();
                            if !running {
                                a.status = AgentStatus::Starting;
                            }
                        });
                        self.run_prompt(&agent_id, p, None);
                    }
                }
            }
            Err(e) => {
                let err_msg = crate::redact::redact_secrets(&format!("{e:#}")).into_owned();
                let state = AutoReviewState {
                    status: AutoReviewStatus::Failed,
                    reviewer_provider: reviewer.provider,
                    reviewer_model: reviewer.model.clone(),
                    summary: format!("Review with {} failed.", reviewer.label),
                    findings: Vec::new(),
                    checked_at: now_ms(),
                    auto_fix_sent: prev_fix_sent,
                    error: Some(err_msg.clone()),
                    diff_fingerprint: Some(snapshot.fingerprint),
                };
                let _ = self.update_agent(&agent_id, |a| {
                    a.auto_review_state = Some(state);
                });
                self.ctx(&agent_id).notice(
                    NoticeLevel::Warn,
                    format!(
                        "Cross-provider review ({}) failed: {err_msg}",
                        reviewer.label
                    ),
                );
            }
        }
    }

    fn claim_review(&self, agent_id: &str) -> Option<ReviewClaim> {
        let mut running = self.inner.running_reviews.lock();
        if !running.insert(agent_id.to_string()) {
            return None;
        }
        Some(ReviewClaim {
            core: self.clone(),
            agent_id: agent_id.to_string(),
        })
    }

    /// Resolves which provider/account/custom agent runs the review.
    fn resolve_reviewer(
        &self,
        agent: &Agent,
        cfg: &AutoReviewConfig,
        settings: &Settings,
    ) -> Result<ResolvedReviewer> {
        let model = cfg
            .model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string);

        // Explicit custom agent configured.
        if cfg.provider == Some(Provider::Custom) || cfg.custom_agent_id.is_some() {
            let def = self.custom_agent_def(cfg.custom_agent_id.as_deref())?;
            if !def.enabled {
                bail!(
                    "The reviewer custom agent \u{201c}{}\u{201d} is disabled",
                    def.name
                );
            }
            let label = match model.as_deref() {
                Some(m) => format!("{} · {m}", def.name),
                None => def.name.clone(),
            };
            return Ok(ResolvedReviewer {
                provider: Provider::Custom,
                account: None,
                custom_def: Some(def),
                model,
                label,
            });
        }

        // Explicit account id configured.
        if let Some(acc_id) = cfg
            .account_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let acc = self.account(acc_id).map_err(|_| {
                anyhow!("The configured reviewer account no longer exists. Pick another in Project settings → Auto-review.")
            })?;
            if let Some(p) = cfg.provider {
                if acc.provider != p {
                    bail!("The configured reviewer account belongs to a different provider");
                }
            }
            let label = match model.as_deref() {
                Some(m) => format!("{} · {m}", acc.provider.display_name()),
                None => acc.provider.display_name().to_string(),
            };
            return Ok(ResolvedReviewer {
                provider: acc.provider,
                account: Some(acc),
                custom_def: None,
                model,
                label,
            });
        }

        // Explicit provider without account id: pick best account for that provider.
        if let Some(provider) = cfg.provider {
            let acc = self.best_account_for(provider).ok_or_else(|| {
                anyhow!(
                    "No account configured for reviewer provider {}. Add one in Settings → Providers.",
                    provider.display_name()
                )
            })?;
            let label = match model.as_deref() {
                Some(m) => format!("{} · {m}", provider.display_name()),
                None => provider.display_name().to_string(),
            };
            return Ok(ResolvedReviewer {
                provider,
                account: Some(acc),
                custom_def: None,
                model,
                label,
            });
        }

        // Auto-select: pick a DIFFERENT provider than the agent's own provider.
        let preference: &[Provider] = match agent.provider {
            Provider::Claude => &[Provider::Codex, Provider::Gemini, Provider::Grok],
            Provider::Codex => &[Provider::Claude, Provider::Gemini, Provider::Grok],
            Provider::Gemini => &[Provider::Claude, Provider::Codex, Provider::Grok],
            Provider::Grok => &[Provider::Claude, Provider::Codex, Provider::Gemini],
            Provider::Local | Provider::Custom | Provider::Opencode | Provider::Cursor => &[
                Provider::Claude,
                Provider::Codex,
                Provider::Gemini,
                Provider::Grok,
            ],
        };
        for &p in preference {
            if let Some(acc) = self.best_account_for(p) {
                let installed =
                    providers::launch_spec_for_account(p, Some(&acc), &self.inner.data_dir).is_ok();
                if installed {
                    let label = match model.as_deref() {
                        Some(m) => format!("{} · {m}", p.display_name()),
                        None => p.display_name().to_string(),
                    };
                    return Ok(ResolvedReviewer {
                        provider: p,
                        account: Some(acc),
                        custom_def: None,
                        model,
                        label,
                    });
                }
            }
        }
        // Fallback to first enabled custom agent if no built-in cross-provider CLI is installed.
        if let Some(def) = settings.custom_agents.iter().find(|c| c.enabled).cloned() {
            let label = match model.as_deref() {
                Some(m) => format!("{} · {m}", def.name),
                None => def.name.clone(),
            };
            return Ok(ResolvedReviewer {
                provider: Provider::Custom,
                account: None,
                custom_def: Some(def),
                model,
                label,
            });
        }

        bail!("No other reviewer provider is installed. Pick a reviewer in Project settings → Auto-review.")
    }

    fn best_account_for(&self, provider: Provider) -> Option<Account> {
        let accounts = self.inner.accounts.lock();
        let mut candidates: Vec<Account> = accounts
            .values()
            .filter(|a| a.provider == provider)
            .cloned()
            .collect();
        candidates.sort_by_key(|a| {
            let logged_in = a.status.as_ref().is_some_and(|s| s.logged_in);
            (!logged_in, !a.is_system, a.created_at)
        });
        candidates.into_iter().next()
    }

    /// Invokes the chosen reviewer in a fresh isolated temporary directory so it can NEVER
    /// modify the agent's worktree files.
    async fn invoke_reviewer(&self, reviewer: &ResolvedReviewer, prompt: &str) -> Result<String> {
        let scratch_path = self
            .inner
            .data_dir
            .join("auto-review-scratch")
            .join(uuid::Uuid::new_v4().simple().to_string());
        std::fs::create_dir_all(&scratch_path)?;
        let _scratch = ScratchDir(scratch_path.clone());

        let children_file = self.inner.data_dir.join("children.json");

        let (program, args, env, env_remove, effective_model) = match reviewer.provider {
            Provider::Custom => {
                let def = reviewer
                    .custom_def
                    .as_ref()
                    .ok_or_else(|| anyhow!("Custom reviewer definition missing"))?;
                let (program, args) = custom_launch(def)?;
                let env: Vec<(String, String)> = def
                    .env
                    .iter()
                    .map(|e| (e.name.clone(), e.value.clone()))
                    .collect();
                (program, args, env, Vec::new(), reviewer.model.clone())
            }
            provider => {
                let account = reviewer
                    .account
                    .as_ref()
                    .ok_or_else(|| anyhow!("Reviewer account missing"))?
                    .clone();
                if let Some(dir) = account.config_dir.as_deref().filter(|_| !account.is_system) {
                    std::fs::create_dir_all(dir)?;
                }
                let mut local_model = reviewer.model.clone();
                if provider == Provider::Local {
                    let server = account.local.clone().ok_or_else(|| {
                        anyhow!("This reviewer account has no model server configured")
                    })?;
                    let acc_id = account.id.clone();
                    let probe = tokio::task::spawn_blocking(move || {
                        let key = accounts::get_api_key(&acc_id);
                        local::probe(&server, key.as_deref())
                    })
                    .await
                    .map_err(|e| anyhow!("{e}"))?;
                    if !probe.running {
                        bail!(probe
                            .error
                            .unwrap_or_else(|| "The reviewer model server isn't running".into()));
                    }
                    if local_model.is_none() {
                        local_model = probe.models.first().map(|m| m.id.clone());
                    }
                }

                let (program, mut args) = providers::launch_spec_for_account(
                    provider,
                    Some(&account),
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
                .map_err(|e| anyhow!("reading reviewer credentials: {e}"))?;

                if provider == Provider::Local {
                    let server = account.local.as_ref().ok_or_else(|| {
                        anyhow!("This reviewer account has no model server configured")
                    })?;
                    args.extend(local::codex_override_args(
                        server,
                        local_model.as_deref(),
                        has_key,
                    ));
                }
                if provider == Provider::Gemini {
                    crate::gemini::prepare_home(&account);
                }
                (program, args, acct_env, acct_env_remove, local_model)
            }
        };

        match reviewer.provider {
            Provider::Codex | Provider::Local => {
                codex::one_shot_prompt(
                    &program,
                    &args,
                    &scratch_path,
                    &env,
                    &env_remove,
                    Some(&children_file),
                    REVIEWER_SYSTEM_PROMPT,
                    effective_model.as_deref(),
                    prompt,
                    REVIEW_TIMEOUT,
                )
                .await
            }
            Provider::Claude
            | Provider::Grok
            | Provider::Gemini
            | Provider::Opencode
            | Provider::Cursor
            | Provider::Custom => {
                acp::one_shot_prompt(
                    &program,
                    &args,
                    &scratch_path,
                    &env,
                    &env_remove,
                    Some(&children_file),
                    REVIEWER_SYSTEM_PROMPT,
                    effective_model.as_deref(),
                    prompt,
                    REVIEW_TIMEOUT,
                )
                .await
            }
        }
    }
}
