# Kvistcode — Future Roadmap & Todo

Prioritized backlog from comparing Kvistcode (as of v0.7.4) against T3 Code, Claude Code Desktop, Cursor, Conductor, Superset, and Vibe Kanban.

Each item is ordered by payoff-to-effort ratio and fit with Kvistcode's local-first, multi-provider architecture.

---

## High Priority

### 1. Worktree setup & teardown hooks
Fresh worktrees currently run only `git worktree add`, leaving out `.env` / untracked config files, installed dependencies, and per-worktree dev server ports.

- [x] Add per-project worktree setup config in Settings (files/globs to copy from the main checkout, setup command after creation, teardown command before removal)
- [x] Optional repo-level config file (`.kvistcode.json`) with read-only fallback support for `.cursor/worktrees.json` and `conductor.json`
- [x] Security gate: prompt once per repo/script hash before running repo-defined commands (never execute untrusted repo scripts silently)
- [x] Surface setup progress and failure output on the agent card / timeline instead of failing silently
- **Touchpoints:** `crates/kvistcode-core/src/git.rs` (`create_worktree`, `remove_worktree`), `crates/kvistcode-core/src/project_config.rs`, `apps/desktop/src/components/settings/`

### 2. Open in Editor + lightweight Files tab
`open_path` currently opens only the OS file manager, and the workspace panel only has Terminal and Scripts.

- [x] Detect installed editors (VS Code, Cursor, Zed, JetBrains IDEs, Neovim/terminal) and remember the user's preferred editor
- [x] Add "Open in `<Editor>`" in the agent header / context menu and on `file:line` links in timelines and diffs
- [x] Add a lightweight **Files** tab to `apps/desktop/src/components/workspace/tabs.ts`: project file tree, syntax-highlighted viewer, and basic edit/save for quick manual fixes (keep it light—do not turn Kvistcode into an IDE)
- **Touchpoints:** `apps/desktop/src-tauri/src/commands.rs` (`open_path`), `apps/desktop/src/components/workspace/tabs.ts`, `crates/kvistcode-core/src/files.rs`

### 3. Per-turn diffs & Thread Forking
Kvistcode already snapshots a git ref before every user message (`refs/kvistcode/*`), making turn-scoped diffs and branching relatively small lifts.

- [x] Add per-turn diff inspection (`checkpoint[n-1]..checkpoint[n]`) directly from a turn's summary / "Worked for…" line
- [x] Add **Fork from here**: create a new worktree/branch at a chosen checkpoint and launch a parallel agent seeded with the conversation context (same or different provider)
- [x] Optional **Side-chat (`/btw`)**: ask a quick non-interrupting question against the same workspace while a main turn is still running
- **Touchpoints:** `crates/kvistcode-core/src/checkpoints.rs`, `crates/kvistcode-core/src/manager.rs`, `apps/desktop/src/components/timeline/`, `apps/desktop/src/components/ReviewSheet.tsx`

### 4. Automatic cross-provider review ("Review before Needs you")
Turn Kvistcode's multi-provider accounts and Handoff into a local, BYOS equivalent of Cursor Bugbot.

- [x] Add an optional per-project / per-agent toggle: when an agent finishes with code changes, automatically run a review pass with a chosen second provider/model (e.g., Codex reviewing Claude's diff)
- [x] Display review findings directly on `ReviewSheet.tsx` (as suggested line comments) and on the Mission Control "Needs you" card
- [x] Optional "send critical findings back once" loop with clear budget/turn caps
- **Touchpoints:** `crates/kvistcode-core/src/manager.rs`, `apps/desktop/src/components/ReviewSheet.tsx`, `apps/desktop/src/components/mission/`

### 5. Expose Kvistcode as a local MCP server
Agents in Kvistcode can consume MCP servers today, but cannot inspect or coordinate sibling Kvistcode agents across providers.

- [x] Add an opt-in local-only MCP endpoint (e.g., on loopback or stdio bridge) with explicit permission controls
- [x] Expose safe fleet tools: `list_agents`, `get_agent_status`, `get_diff`, `read_summary`, `spawn_agent`, `send_message`
- [x] Allow one provider (e.g., Claude) to delegate a review or parallel subtask to another provider (e.g., Codex or Gemini) in its own worktree, and allow external terminal agents to inspect running Kvistcode threads
- **Touchpoints:** `crates/kvistcode-core/src/mcp.rs`, `apps/desktop/src-tauri/src/remote.rs`, `crates/kvistcode-core/src/manager.rs`

---

## Safety Pick

### 6. Optional OS / container sandboxing for unattended runs
Git worktrees isolate repository files, not the broader filesystem or network—especially important in "Run without asking" mode (which auto-approves sandbox escapes as of v0.7.4).

- [x] Optional per-project sandbox mode using Bubblewrap (`bwrap`) on Linux and/or Docker containers cross-platform
- [x] Restrict write access to the agent's worktree + temp dir and make network access opt-in per project
- [x] Show a clear sandbox badge in the agent header when active

---

## Medium / Later Ideas

- [ ] **Import & resume external CLI sessions:** Pull sessions started in a regular terminal (`claude`, `codex`, etc.) into Kvistcode, or open a Kvistcode session back in a terminal CLI
- [ ] **Inbound event triggers:** Start agents from inbound webhooks, GitHub PR mentions / CI failures, Linear issues, Sentry errors, or Slack messages (complementing existing cron Schedules and outbound alerts)
- [ ] **Project task board (Kanban view):** Optional board view over agents/issues per project (Backlog → Working → Needs review → Merged)
- [ ] **First-class providers:** Promote popular ACP agents (OpenCode, Cursor CLI, etc.) from generic custom ACP to built-in install/auth/model-picker/usage parity
- [x] **Interactive inline artifacts:** Render sandboxed HTML/SVG/chart artifacts inline in the conversation timeline
- [ ] **Package-manager distribution & macOS:** Winget, Scoop, Homebrew cask, and revisiting macOS release builds (currently held on v0.6.1)

---

## Explicitly Deferred (for now)

- **Hosted relay tunnel & native iOS/Android apps (T3 Connect style):** Tailscale + the embedded phone PWA already cover remote control with zero server infrastructure; revisit only if Tailscale setup becomes a real barrier.
- **Headless remote daemon & SSH workspaces:** Valuable for driving a remote dev box from a laptop, but requires splitting the single-process architecture (`kvistcoded` + client); keep as a dedicated future milestone once the local workflow is airtight.
