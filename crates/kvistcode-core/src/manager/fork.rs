//! Thread forking: branch a new parallel agent from any checkpoint (or current state)
//! on its own git worktree, seeded with the conversation context up to that point.

use super::*;

/// Starts an agent as an independent fork of `forked_from`, rooted at `commit`.
/// `preamble` carries the conversation context up to the fork point for the model.
pub(super) struct ForkSeed {
    pub commit: String,
    pub base_branch: Option<String>,
    pub forked_from: String,
    pub preamble: String,
}

fn nonblank(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Conversation items up to (and excluding, for turn checkpoints) the chosen checkpoint.
pub(super) fn history_up_to<'a>(
    items: &'a [TimelineItem],
    checkpoint: &crate::checkpoints::Checkpoint,
) -> &'a [TimelineItem] {
    if let Some(item_id) = checkpoint.item_id.as_deref() {
        if let Some(pos) = items.iter().position(|it| it.id == item_id) {
            return &items[..pos];
        }
    }
    let cut = items
        .iter()
        .position(|it| it.ts > checkpoint.created_at)
        .unwrap_or(items.len());
    &items[..cut]
}

impl Core {
    /// Creates a new parallel agent on its own git worktree rooted at `checkpoint_id`
    /// (or a fresh snapshot of the source agent's current files when `checkpoint_id` is None),
    /// seeded with the conversation history up to that point.
    pub async fn fork_agent(
        &self,
        agent_id: &str,
        checkpoint_id: Option<&str>,
        req: ForkAgentRequest,
    ) -> Result<Agent> {
        let src = self.agent(agent_id)?;
        if self.is_shutting_down() {
            bail!(SHUTTING_DOWN);
        }
        let project = PathBuf::from(&src.project_path);
        let repo = git::repo_info(&project)
            .await
            .ok_or_else(|| anyhow!("Forking needs a git repository"))?;

        let all_items = self.items(&src.id).unwrap_or_default();
        let ck_id = checkpoint_id.map(str::trim).filter(|s| !s.is_empty());
        let (commit, history_len, fork_label) = match ck_id {
            Some(id) => {
                let ck = self
                    .inner
                    .db
                    .checkpoints(&src.id)?
                    .into_iter()
                    .find(|c| c.id == id)
                    .ok_or_else(|| anyhow!("That checkpoint no longer exists"))?;
                crate::checkpoints::verify_commit(&src, &ck.commit).await?;
                let slice = history_up_to(&all_items, &ck);
                (ck.commit, slice.len(), format!("checkpoint #{}", ck.seq))
            }
            None => {
                let commit = crate::checkpoints::snapshot_current_commit(&src).await?;
                (commit, all_items.len(), "current state".to_string())
            }
        };

        let provider = req.provider.unwrap_or(src.provider);
        let accounts = self.accounts()?;
        let exists = |id: &str, p: Provider| accounts.iter().any(|a| a.id == id && a.provider == p);

        let account_id = if let Some(id) = nonblank(&req.account_id) {
            if !exists(id, provider) {
                bail!("That account does not match the chosen provider");
            }
            id.to_string()
        } else if provider == src.provider && exists(&src.account_id, provider) {
            src.account_id.clone()
        } else {
            accounts
                .iter()
                .find(|a| a.provider == provider && a.status.as_ref().is_some_and(|s| s.logged_in))
                .or_else(|| accounts.iter().find(|a| a.provider == provider))
                .map(|a| a.id.clone())
                .ok_or_else(|| {
                    anyhow!(
                        "No account configured for {}. Add one in Accounts first.",
                        provider.display_name()
                    )
                })?
        };

        let custom_agent_id = if provider == Provider::Custom {
            let settings = self.settings()?;
            let usable = |id: &str| {
                settings
                    .custom_agents
                    .iter()
                    .any(|c| c.id == id && c.enabled)
            };
            nonblank(&req.custom_agent_id)
                .filter(|id| usable(id))
                .map(str::to_string)
                .or_else(|| {
                    (src.provider == Provider::Custom)
                        .then(|| src.custom_agent_id.clone())
                        .flatten()
                        .filter(|id| usable(id))
                })
                .or_else(|| {
                    settings
                        .custom_agents
                        .iter()
                        .find(|c| c.enabled)
                        .map(|c| c.id.clone())
                })
        } else {
            None
        };

        let model = nonblank(&req.model).map(str::to_string).or_else(|| {
            if provider == src.provider {
                src.model
                    .clone()
                    .or_else(|| src.config_choices.get("model").cloned())
            } else {
                None
            }
        });

        let mut config_choices = BTreeMap::new();
        if provider != Provider::Local {
            if let Some(m) = &model {
                config_choices.insert("model".to_string(), m.clone());
            }
        }

        let history = &all_items[..history_len];
        let mut preamble = format!(
            "You are working in an independent forked session branched from \u{201c}{}\u{201d} ({fork_label}, originally {}).\n\
             Your working directory is a separate git worktree and branch starting from the exact files at that point. \
             Changes you make here are completely isolated from the original session.\n",
            src.title,
            src.provider.display_name(),
        );
        let has_messages = history.iter().any(|it| {
            matches!(
                it.body,
                ItemBody::UserMessage { .. } | ItemBody::AgentMessage { .. }
            )
        });
        if has_messages {
            let transcript = self.transcript_of(history, 12_000);
            match self.condense_handoff(&transcript).await {
                Some(summary) => {
                    preamble.push_str("\nSummary of the conversation up to the fork point:\n\n");
                    preamble.push_str(&summary);
                }
                None => {
                    preamble.push_str("\nConversation up to the fork point:\n\n");
                    preamble.push_str(&transcript);
                }
            }
        }

        let instruction = nonblank(&req.instruction).unwrap_or("").to_string();
        let title = nonblank(&req.title)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{} (fork)", src.title));

        let create_req = CreateAgentRequest {
            provider,
            account_id,
            project_path: src.project_path.clone(),
            prompt: instruction,
            title: Some(title),
            worktree: true,
            model,
            group_id: src.group_id.clone(),
            custom_agent_id,
            permission_mode: src.permission_mode.clone(),
            attachments: Vec::new(),
            config_choices,
        };
        let seed = ForkSeed {
            commit,
            base_branch: src.base_branch.clone().or(repo.branch),
            forked_from: src.id.clone(),
            preamble,
        };
        let forked = self
            .create_agent_inner(create_req, None, None, None, Some(seed))
            .await?;
        self.ctx(&forked.id).notice(
            NoticeLevel::Info,
            format!(
                "Forked from \u{201c}{}\u{201d} ({fork_label}). Working on an isolated branch.",
                src.title
            ),
        );
        Ok(forked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_item(id: &str, ts: i64, text: &str) -> TimelineItem {
        TimelineItem {
            id: id.into(),
            agent_id: "a1".into(),
            seq: ts,
            ts,
            body: ItemBody::UserMessage {
                text: text.into(),
                attachments: vec![],
                automatic: false,
                mid_turn: false,
            },
        }
    }

    fn ck(item_id: Option<&str>, created_at: i64) -> crate::checkpoints::Checkpoint {
        crate::checkpoints::Checkpoint {
            id: "ck1".into(),
            agent_id: "a1".into(),
            seq: 2,
            item_id: item_id.map(str::to_string),
            created_at,
            kind: "turn".into(),
            tree: "t".into(),
            commit: "c".into(),
        }
    }

    #[test]
    fn history_up_to_slices_before_checkpoint_item() {
        let items = vec![
            user_item("u1", 10, "first"),
            user_item("u2", 20, "second"),
            user_item("u3", 30, "third"),
        ];
        let slice = history_up_to(&items, &ck(Some("u2"), 20));
        assert_eq!(slice.len(), 1);
        assert_eq!(slice[0].id, "u1");

        // Safety checkpoint without item_id falls back to timestamp.
        let slice = history_up_to(&items, &ck(None, 25));
        assert_eq!(slice.len(), 2);
        assert_eq!(slice[1].id, "u2");
    }
}
