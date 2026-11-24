//! Importing a session that was started in a plain terminal (see `crate::external_sessions`).

use super::*;
use crate::external_sessions::{self as ext, ImportRequest, Message, Role};

/// What an imported agent starts from.
pub(super) struct ImportSeed {
    pub session_id: String,
    pub messages: Vec<Message>,
}

/// Puts the imported transcript at the top of the agent's timeline, with a plain note on what
/// happens next. The messages are history only: if the provider can't reopen the session, the
/// next prompt carries them as context (the usual briefing for a fresh session).
pub(super) fn seed_timeline(ctx: &AgentCtx, seed: &ImportSeed) {
    for m in &seed.messages {
        ctx.push(match m.role {
            Role::User => ItemBody::UserMessage {
                text: m.text.clone(),
                attachments: Vec::new(),
                mid_turn: false,
                automatic: false,
            },
            Role::Assistant => ItemBody::AgentMessage {
                text: m.text.clone(),
                images: Vec::new(),
            },
        });
    }
    ctx.notice(
        NoticeLevel::Info,
        "Imported from a terminal session. Kvistcode reopens that session to continue it. \
         If it can't, this history is read-only and your next message sends it along as context.",
    );
}

impl Core {
    /// Sessions of `provider` started in a terminal in `project_path`, newest first.
    pub fn list_external_sessions(
        &self,
        provider: Provider,
        project_path: &str,
    ) -> Vec<ext::ExternalSession> {
        let Some(home) = providers::home_dir() else {
            return Vec::new();
        };
        let project = providers::expand_home(project_path);
        ext::list_sessions(&home, provider, &project)
    }

    /// Creates an agent that continues a terminal session. It works in the project folder
    /// itself (the CLI finds a session by folder), not a worktree.
    pub async fn import_external_session(&self, req: ImportRequest) -> Result<Agent> {
        if !ext::supported(req.provider) {
            bail!("Only Claude Code and Codex sessions can be imported");
        }
        if !ext::valid_id(&req.session_id) {
            bail!("That isn't a valid session id");
        }
        let home =
            providers::home_dir().ok_or_else(|| anyhow!("Couldn't find your home folder"))?;
        let project = providers::expand_home(&req.project_path);
        let known = ext::list_sessions(&home, req.provider, &project)
            .into_iter()
            .find(|s| s.id == req.session_id)
            .ok_or_else(|| anyhow!("That session is no longer there"))?;
        let messages = ext::read_transcript(&home, req.provider, &project, &req.session_id);
        let create = CreateAgentRequest {
            provider: req.provider,
            account_id: req.account_id,
            project_path: req.project_path,
            prompt: String::new(),
            title: Some(known.title).filter(|t| !t.is_empty()),
            worktree: false,
            model: None,
            group_id: None,
            custom_agent_id: None,
            permission_mode: None,
            attachments: Vec::new(),
            config_choices: Default::default(),
        };
        self.create_agent_inner(
            create,
            None,
            None,
            None,
            Some(ImportSeed {
                session_id: req.session_id,
                messages,
            }),
        )
        .await
    }
}
