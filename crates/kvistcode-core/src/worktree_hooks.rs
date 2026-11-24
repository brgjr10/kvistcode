//! Per-project worktree setup and teardown hooks.
//!
//! Supports user-configured hooks in Settings (`Settings.project_worktrees`) as well as
//! read-only repo-level config files in priority order:
//! 1. `.kvistcode.json`
//! 2. `.cursor/worktrees.json`
//! 3. `conductor.json`
//!
//! Security model:
//! - User-configured commands in Kvistcode Settings run without an extra prompt.
//! - Repo-defined shell commands NEVER run silently: callers must check `needs_trust`
//!   and persist the repo's `script_hash` in `Settings.trusted_worktree_hooks` only after
//!   explicit user approval. Changing a repo command changes its hash and prompts again.
//! - File copying (`copy_files`) stays strictly inside the project root (no `..`, no
//!   `.git`, no symlinks escaping the project root) and never overwrites existing files
//!   in the worktree.
//! - All command output is redacted with `crate::redact::redact_secrets` before surfacing.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::model::Settings;
use crate::redact::redact_secrets;

pub const SETUP_TIMEOUT: Duration = Duration::from_secs(300);
pub const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(60);

const MAX_REPO_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_PATTERNS: usize = 64;
const MAX_PATTERN_LEN: usize = 260;
const MAX_COMMAND_LEN: usize = 8 * 1024;
const MAX_COPY_FILES: usize = 500;
const MAX_SINGLE_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_TOTAL_COPY_BYTES: u64 = 200 * 1024 * 1024;
const MAX_WALK_ENTRIES: usize = 20_000;
const MAX_WALK_DEPTH: usize = 8;
const MAX_STREAM_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_SURFACED_OUTPUT_CHARS: usize = 4_000;

/// User-configured worktree hooks for a single project (persisted in `Settings`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectWorktreeConfig {
    /// Relative paths or glob patterns (e.g. `.env`, `.env.local`, `config/*.local.json`)
    /// copied from the main checkout into each new worktree.
    pub copy_files: Vec<String>,
    /// Shell command run inside the new worktree after files are copied and before the agent starts.
    pub setup_command: Option<String>,
    /// Shell command run inside the worktree right before it is removed when deleting an agent.
    pub teardown_command: Option<String>,
}

impl ProjectWorktreeConfig {
    pub fn is_empty(&self) -> bool {
        self.copy_files.iter().all(|p| p.trim().is_empty())
            && trim_opt(&self.setup_command).is_none()
            && trim_opt(&self.teardown_command).is_none()
    }

    pub fn problem(&self) -> Option<String> {
        if self.copy_files.len() > MAX_PATTERNS {
            return Some(format!("At most {MAX_PATTERNS} copy-file patterns allowed"));
        }
        for pat in &self.copy_files {
            if pat.len() > MAX_PATTERN_LEN {
                return Some("A copy-file pattern is too long".into());
            }
            if pat.contains('\0') || pat.contains('\n') || pat.contains('\r') {
                return Some("Copy-file patterns cannot contain control characters".into());
            }
        }
        for (name, cmd) in [
            ("Setup command", &self.setup_command),
            ("Teardown command", &self.teardown_command),
        ] {
            if let Some(c) = cmd {
                if c.len() > MAX_COMMAND_LEN {
                    return Some(format!("{name} is too long (max {MAX_COMMAND_LEN} bytes)"));
                }
                if c.contains('\0') {
                    return Some(format!("{name} cannot contain null bytes"));
                }
            }
        }
        None
    }

    /// Normalizes stored values; returns true if anything was dropped or clamped.
    pub fn sanitize(&mut self) -> bool {
        let mut changed = false;
        let orig_len = self.copy_files.len();
        let mut seen = HashSet::new();
        let mut clean = Vec::new();
        for pat in self.copy_files.drain(..) {
            let trimmed = pat.trim();
            if trimmed.is_empty()
                || trimmed.len() > MAX_PATTERN_LEN
                || trimmed.contains('\0')
                || trimmed.contains('\n')
                || trimmed.contains('\r')
            {
                changed = true;
                continue;
            }
            if seen.insert(trimmed.to_string()) {
                if clean.len() < MAX_PATTERNS {
                    clean.push(trimmed.to_string());
                } else {
                    changed = true;
                }
            } else {
                changed = true;
            }
        }
        if clean.len() != orig_len {
            changed = true;
        }
        self.copy_files = clean;

        for cmd in [&mut self.setup_command, &mut self.teardown_command] {
            if let Some(c) = cmd.clone() {
                let t = c.trim();
                if t.is_empty() || t.len() > MAX_COMMAND_LEN || t.contains('\0') {
                    *cmd = None;
                    changed = true;
                } else if t != c {
                    *cmd = Some(t.to_string());
                    changed = true;
                }
            }
        }
        changed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeHookSource {
    Settings,
    KvistcodeJson,
    CursorJson,
    ConductorJson,
}

/// Read-only view of a repo-level worktree config file discovered inside a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoWorktreeConfig {
    pub source: WorktreeHookSource,
    /// Relative path inside the project root (e.g. `.kvistcode.json`).
    pub source_path: String,
    #[serde(default)]
    pub copy_files: Vec<String>,
    #[serde(default)]
    pub setup_command: Option<String>,
    #[serde(default)]
    pub teardown_command: Option<String>,
    /// SHA-256 hex digest of the repo-defined shell commands; None when the repo defines no commands.
    #[serde(default)]
    pub script_hash: Option<String>,
    /// Whether `script_hash` is currently trusted (always true if `script_hash` is None).
    #[serde(default)]
    pub trusted: bool,
    /// Present when the repo file exists on disk but couldn't be read or parsed safely.
    #[serde(default)]
    pub parse_error: Option<String>,
}

/// Effective worktree hooks for a project, combining Kvistcode Settings and any repo config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedWorktreeHooks {
    pub project_path: String,
    pub settings: ProjectWorktreeConfig,
    pub repo: Option<RepoWorktreeConfig>,
    pub copy_files: Vec<String>,
    pub setup_command: Option<String>,
    pub setup_from_repo: bool,
    pub teardown_command: Option<String>,
    pub teardown_from_repo: bool,
    pub needs_trust: bool,
}

impl ResolvedWorktreeHooks {
    pub fn has_setup_work(&self) -> bool {
        !self.copy_files.is_empty() || self.setup_command.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeSetupStatus {
    Running,
    NeedsTrust,
    Failed,
}

/// Persisted/emitted status of a worktree agent's setup lifecycle.
/// Cleared (`None`) once setup succeeds or is skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSetupState {
    pub status: WorktreeSetupStatus,
    /// Repo config relative path when the command came from a repo file (e.g. `.kvistcode.json`).
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub setup_command: Option<String>,
    #[serde(default)]
    pub teardown_command: Option<String>,
    #[serde(default)]
    pub script_hash: Option<String>,
    /// Short failure reason (e.g. exit code or timeout).
    #[serde(default)]
    pub error: Option<String>,
    /// Redacted tail of stdout/stderr from the failed setup command.
    #[serde(default)]
    pub output: Option<String>,
}

/// Summary returned by [`copy_files`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopySummary {
    pub copied: Vec<String>,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

/// Result of running a setup or teardown shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRunOutcome {
    pub success: bool,
    pub error: Option<String>,
    pub output: String,
}

fn trim_opt(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Normalizes a project path key for lookup in Settings maps.
pub fn norm_project_key(path: &str) -> String {
    let trimmed = path.trim();
    let expanded = crate::providers::expand_home(trimmed);
    let s = expanded.to_string_lossy();
    let stripped = s.trim_end_matches(['/', '\\']);
    if stripped.is_empty() && !s.is_empty() {
        s.into_owned()
    } else {
        stripped.to_string()
    }
}

/// Looks up a value in a project-keyed `BTreeMap` by exact or normalized path.
pub fn map_get<'a, V>(map: &'a BTreeMap<String, V>, project_path: &str) -> Option<&'a V> {
    if let Some(v) = map.get(project_path) {
        return Some(v);
    }
    let want = norm_project_key(project_path);
    if let Some(v) = map.get(&want) {
        return Some(v);
    }
    let want_path = Path::new(&want);
    map.iter()
        .find(|(k, _)| {
            Path::new(&norm_project_key(k))
                .components()
                .eq(want_path.components())
        })
        .map(|(_, v)| v)
}

/// Removes all keys matching `project_path` (exact or normalized) from a project-keyed `BTreeMap`.
pub fn map_remove_matching<V>(map: &mut BTreeMap<String, V>, project_path: &str) {
    let want = norm_project_key(project_path);
    let want_path = Path::new(&want);
    let keys: Vec<String> = map
        .keys()
        .filter(|k| {
            k.as_str() == project_path
                || Path::new(&norm_project_key(k))
                    .components()
                    .eq(want_path.components())
        })
        .cloned()
        .collect();
    for k in keys {
        map.remove(&k);
    }
}

/// Computes the deterministic SHA-256 hash of a repo's worktree shell commands.
/// Returns `None` if neither command is present.
pub fn compute_script_hash(
    source_path: &str,
    setup_command: Option<&str>,
    teardown_command: Option<&str>,
) -> Option<String> {
    let setup = setup_command.map(str::trim).filter(|s| !s.is_empty());
    let teardown = teardown_command.map(str::trim).filter(|s| !s.is_empty());
    if setup.is_none() && teardown.is_none() {
        return None;
    }
    let mut hasher = Sha256::new();
    hasher.update(b"kvistcode-worktree-hooks-v1\0");
    hasher.update(source_path.as_bytes());
    hasher.update(b"\0");
    if let Some(s) = setup {
        hasher.update(s.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(t) = teardown {
        hasher.update(t.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    Some(hex)
}

/// Resolves effective worktree hooks for `project_path`, merging `Settings` and any repo config.
pub fn resolve(project_path: &str, settings: &Settings) -> ResolvedWorktreeHooks {
    let user_cfg = map_get(&settings.project_worktrees, project_path)
        .cloned()
        .unwrap_or_default();
    let mut repo = detect_repo_config(Path::new(project_path));
    if let Some(r) = repo.as_mut() {
        r.trusted = match r.script_hash.as_deref() {
            None => true,
            Some(hash) => map_get(&settings.trusted_worktree_hooks, project_path)
                .is_some_and(|saved| saved == hash),
        };
    }

    let mut copy_files = Vec::new();
    let mut seen = HashSet::new();
    if let Some(r) = &repo {
        for pat in &r.copy_files {
            if seen.insert(pat.clone()) {
                copy_files.push(pat.clone());
            }
        }
    }
    for pat in &user_cfg.copy_files {
        let trimmed = pat.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            copy_files.push(trimmed.to_string());
        }
    }

    let user_setup = trim_opt(&user_cfg.setup_command);
    let user_teardown = trim_opt(&user_cfg.teardown_command);
    let repo_setup = repo.as_ref().and_then(|r| trim_opt(&r.setup_command));
    let repo_teardown = repo.as_ref().and_then(|r| trim_opt(&r.teardown_command));

    let (setup_command, setup_from_repo) = match user_setup {
        Some(cmd) => (Some(cmd), false),
        None => match repo_setup {
            Some(cmd) => (Some(cmd), true),
            None => (None, false),
        },
    };
    let (teardown_command, teardown_from_repo) = match user_teardown {
        Some(cmd) => (Some(cmd), false),
        None => match repo_teardown {
            Some(cmd) => (Some(cmd), true),
            None => (None, false),
        },
    };

    let repo_trusted = repo.as_ref().is_none_or(|r| r.trusted);
    let needs_trust = (setup_from_repo || teardown_from_repo) && !repo_trusted;

    ResolvedWorktreeHooks {
        project_path: project_path.to_string(),
        settings: user_cfg,
        repo,
        copy_files,
        setup_command,
        setup_from_repo,
        teardown_command,
        teardown_from_repo,
        needs_trust,
    }
}

/// Inspects `project_root` for repo-level worktree config files in priority order:
/// `.kvistcode.json`, `.cursor/worktrees.json`, `conductor.json`.
pub fn detect_repo_config(project_root: &Path) -> Option<RepoWorktreeConfig> {
    let candidates = [
        (WorktreeHookSource::KvistcodeJson, ".kvistcode.json"),
        (WorktreeHookSource::CursorJson, ".cursor/worktrees.json"),
        (WorktreeHookSource::ConductorJson, "conductor.json"),
    ];
    let canon_root = std::fs::canonicalize(project_root).ok()?;
    for (source, rel) in candidates {
        let path = project_root.join(rel);
        // Use symlink_metadata so broken or escaping symlinks are caught and reported.
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        return Some(read_and_parse_repo_config(&canon_root, &path, source, rel));
    }
    None
}

fn read_and_parse_repo_config(
    canon_root: &Path,
    path: &Path,
    source: WorktreeHookSource,
    rel: &str,
) -> RepoWorktreeConfig {
    let make_err = |msg: String| RepoWorktreeConfig {
        source,
        source_path: rel.to_string(),
        copy_files: Vec::new(),
        setup_command: None,
        teardown_command: None,
        script_hash: None,
        trusted: true,
        parse_error: Some(msg),
    };

    let Ok(canon_file) = std::fs::canonicalize(path) else {
        return make_err(format!("Couldn't resolve {rel}"));
    };
    if !canon_file.starts_with(canon_root) {
        return make_err(format!("{rel} points outside the project folder"));
    }
    let Ok(meta) = std::fs::metadata(&canon_file) else {
        return make_err(format!("Couldn't read metadata for {rel}"));
    };
    if !meta.is_file() {
        return make_err(format!("{rel} is not a regular file"));
    }
    if meta.len() > MAX_REPO_CONFIG_BYTES {
        return make_err(format!(
            "{rel} is larger than {} KB",
            MAX_REPO_CONFIG_BYTES / 1024
        ));
    }
    let raw = match std::fs::read_to_string(&canon_file) {
        Ok(s) => s,
        Err(e) => return make_err(format!("Couldn't read {rel}: {e}")),
    };
    let val: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return make_err(format!("{rel} is not valid JSON: {e}")),
    };

    let parsed = match source {
        WorktreeHookSource::KvistcodeJson => parse_kvistcode_json(&val),
        WorktreeHookSource::CursorJson => parse_cursor_json(&val),
        WorktreeHookSource::ConductorJson => parse_conductor_json(&val),
        WorktreeHookSource::Settings => Ok(ProjectWorktreeConfig::default()),
    };

    match parsed {
        Ok(mut cfg) => {
            cfg.sanitize();
            let script_hash = compute_script_hash(
                rel,
                cfg.setup_command.as_deref(),
                cfg.teardown_command.as_deref(),
            );
            RepoWorktreeConfig {
                source,
                source_path: rel.to_string(),
                copy_files: cfg.copy_files,
                setup_command: cfg.setup_command,
                teardown_command: cfg.teardown_command,
                trusted: script_hash.is_none(),
                script_hash,
                parse_error: None,
            }
        }
        Err(e) => make_err(format!("Couldn't parse {rel}: {e}")),
    }
}

fn json_cmd(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" && "))
            }
        }
        _ => None,
    }
}

fn json_string_list(val: &serde_json::Value) -> Vec<String> {
    match val {
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                vec![t.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

fn first_field<'a>(
    obj: &'a serde_json::Map<String, serde_json::Value>,
    names: &[&str],
) -> Option<&'a serde_json::Value> {
    names.iter().find_map(|n| obj.get(*n))
}

fn parse_kvistcode_json(val: &serde_json::Value) -> Result<ProjectWorktreeConfig, String> {
    let obj = val.as_object().ok_or("expected a JSON object")?;
    let scope = match obj.get("worktree").or_else(|| obj.get("worktrees")) {
        Some(serde_json::Value::Object(w)) => w,
        Some(_) => return Err("\"worktree\" must be an object".into()),
        None => obj,
    };
    let copy_files = first_field(scope, &["copyFiles", "copy_files", "copy"])
        .map(json_string_list)
        .unwrap_or_default();
    let setup_command =
        first_field(scope, &["setupCommand", "setup_command", "setup"]).and_then(json_cmd);
    let teardown_command = first_field(
        scope,
        &["teardownCommand", "teardown_command", "teardown", "archive"],
    )
    .and_then(json_cmd);
    Ok(ProjectWorktreeConfig {
        copy_files,
        setup_command,
        teardown_command,
    })
}

fn parse_cursor_json(val: &serde_json::Value) -> Result<ProjectWorktreeConfig, String> {
    let obj = val.as_object().ok_or("expected a JSON object")?;
    let platform_key = if cfg!(windows) {
        "setup-worktree-win"
    } else {
        "setup-worktree-unix"
    };
    let setup_command = obj
        .get(platform_key)
        .and_then(json_cmd)
        .or_else(|| obj.get("setup-worktree").and_then(json_cmd));
    Ok(ProjectWorktreeConfig {
        copy_files: Vec::new(),
        setup_command,
        teardown_command: None,
    })
}

fn parse_conductor_json(val: &serde_json::Value) -> Result<ProjectWorktreeConfig, String> {
    let obj = val.as_object().ok_or("expected a JSON object")?;
    let scripts = obj
        .get("scripts")
        .and_then(|v| v.as_object())
        .unwrap_or(obj);
    let setup_command = first_field(scripts, &["setup", "setupCommand"]).and_then(json_cmd);
    let teardown_command =
        first_field(scripts, &["archive", "teardown", "teardownCommand"]).and_then(json_cmd);
    Ok(ProjectWorktreeConfig {
        copy_files: Vec::new(),
        setup_command,
        teardown_command,
    })
}

// ---- safe file & glob copying ----------------------------------------------

fn has_glob_meta(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// Validates a relative path/glob pattern against traversal and `.git`.
fn validate_pattern(raw: &str) -> Result<String, String> {
    let pat = raw.trim().replace('\\', "/");
    let pat = pat.trim_start_matches("./").trim_end_matches('/');
    if pat.is_empty() {
        return Err("empty pattern".into());
    }
    if pat.starts_with('/') || pat.starts_with('~') || pat.contains('\0') {
        return Err(format!("skipped unsafe pattern \"{raw}\""));
    }
    // Reject Windows drive prefixes like C:...
    if pat.len() >= 2 && pat.as_bytes()[1] == b':' {
        return Err(format!("skipped unsafe pattern \"{raw}\""));
    }
    for comp in Path::new(pat).components() {
        match comp {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("skipped unsafe pattern \"{raw}\""));
            }
            Component::Normal(seg) if seg == ".git" => {
                return Err(format!("skipped .git pattern \"{raw}\""));
            }
            _ => {}
        }
    }
    Ok(pat.to_string())
}

/// Matches a normalized forward-slash relative path against a glob pattern supporting
/// `*` (within a segment), `?` (single non-slash char), and `**` (zero or more segments).
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match_segments(&pat_segs, &path_segs)
}

fn match_segments(pat: &[&str], path: &[&str]) -> bool {
    if pat.is_empty() {
        return path.is_empty();
    }
    if pat[0] == "**" {
        // Collapse consecutive ** segments to avoid exponential backtracking.
        let mut next = 1;
        while next < pat.len() && pat[next] == "**" {
            next += 1;
        }
        let rest = &pat[next..];
        if rest.is_empty() {
            return true;
        }
        for i in 0..=path.len() {
            if match_segments(rest, &path[i..]) {
                return true;
            }
        }
        return false;
    }
    if path.is_empty() {
        return false;
    }
    match_segment(pat[0], path[0]) && match_segments(&pat[1..], &path[1..])
}

fn match_segment(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            pi += 1;
            star_ti = ti;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Copies files matching `patterns` from `project_root` into `worktree_root`.
/// Never escapes `project_root`, skips `.git`, and never overwrites existing files.
pub fn copy_files(project_root: &Path, worktree_root: &Path, patterns: &[String]) -> CopySummary {
    let mut summary = CopySummary::default();
    if patterns.is_empty() {
        return summary;
    }
    let Ok(canon_project) = std::fs::canonicalize(project_root) else {
        summary
            .warnings
            .push("Couldn't resolve project folder for copying worktree files.".into());
        return summary;
    };
    let Ok(canon_worktree) = std::fs::canonicalize(worktree_root) else {
        summary
            .warnings
            .push("Couldn't resolve worktree folder for copying files.".into());
        return summary;
    };
    if canon_project == canon_worktree {
        return summary;
    }

    let mut literals = Vec::new();
    let mut globs = Vec::new();
    for raw in patterns {
        match validate_pattern(raw) {
            Ok(pat) => {
                if has_glob_meta(&pat) {
                    globs.push(pat);
                } else {
                    literals.push(pat);
                }
            }
            Err(w) => summary.warnings.push(w),
        }
    }

    let mut matched: Vec<PathBuf> = Vec::new();
    let mut seen_rel = HashSet::new();

    // 1. Exact files or directories.
    for rel in &literals {
        let src = canon_project.join(rel);
        let Ok(meta) = std::fs::symlink_metadata(&src) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            // Follow only if target resolves inside canon_project.
            let Ok(target) = std::fs::canonicalize(&src) else {
                summary
                    .warnings
                    .push(format!("Skipped broken symlink \"{rel}\""));
                continue;
            };
            if !target.starts_with(&canon_project) {
                summary
                    .warnings
                    .push(format!("Skipped symlink outside project: \"{rel}\""));
                continue;
            }
            if target.is_file() && seen_rel.insert(PathBuf::from(rel)) {
                matched.push(PathBuf::from(rel));
            }
        } else if meta.is_file() {
            if seen_rel.insert(PathBuf::from(rel)) {
                matched.push(PathBuf::from(rel));
            }
        } else if meta.is_dir() {
            collect_dir_files(
                &canon_project,
                &src,
                Path::new(rel),
                0,
                &mut seen_rel,
                &mut matched,
                &mut summary.warnings,
            );
        }
    }

    // 2. Glob patterns (single bounded walk of the project root).
    if !globs.is_empty() {
        let include_node_modules = globs.iter().any(|g| g.contains("node_modules"));
        walk_for_globs(
            &canon_project,
            &globs,
            include_node_modules,
            &mut seen_rel,
            &mut matched,
        );
    }

    let mut total_bytes: u64 = 0;
    for rel in matched {
        if summary.copied.len() >= MAX_COPY_FILES {
            summary.warnings.push(format!(
                "Stopped after copying {MAX_COPY_FILES} files (limit reached)."
            ));
            break;
        }
        let src = canon_project.join(&rel);
        let Ok(canon_src) = std::fs::canonicalize(&src) else {
            continue;
        };
        if !canon_src.starts_with(&canon_project) {
            summary.warnings.push(format!(
                "Skipped symlink outside project: \"{}\"",
                rel.display()
            ));
            continue;
        }
        let Ok(meta) = std::fs::metadata(&canon_src) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > MAX_SINGLE_FILE_BYTES {
            summary.warnings.push(format!(
                "Skipped \"{}\" (larger than {} MB)",
                rel.display(),
                MAX_SINGLE_FILE_BYTES / (1024 * 1024)
            ));
            continue;
        }
        if total_bytes.saturating_add(meta.len()) > MAX_TOTAL_COPY_BYTES {
            summary.warnings.push(format!(
                "Stopped copying files after {} MB limit.",
                MAX_TOTAL_COPY_BYTES / (1024 * 1024)
            ));
            break;
        }

        let dst = canon_worktree.join(&rel);
        if std::fs::symlink_metadata(&dst).is_ok() {
            summary.skipped += 1;
            continue;
        }
        if let Some(parent) = dst.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                summary.warnings.push(format!(
                    "Couldn't create folder for \"{}\": {e}",
                    rel.display()
                ));
                continue;
            }
            // Verify parent stays inside worktree (guards against symlinks in the worktree).
            let Ok(canon_parent) = std::fs::canonicalize(parent) else {
                continue;
            };
            if !canon_parent.starts_with(&canon_worktree) {
                summary.warnings.push(format!(
                    "Skipped destination outside worktree: \"{}\"",
                    rel.display()
                ));
                continue;
            }
        }
        match std::fs::copy(&canon_src, &dst) {
            Ok(bytes) => {
                total_bytes = total_bytes.saturating_add(bytes);
                summary
                    .copied
                    .push(rel.to_string_lossy().replace('\\', "/"));
            }
            Err(e) => {
                summary
                    .warnings
                    .push(format!("Couldn't copy \"{}\": {e}", rel.display()));
            }
        }
    }

    summary
}

fn collect_dir_files(
    canon_project: &Path,
    dir: &Path,
    rel_dir: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) {
    if depth > MAX_WALK_DEPTH || out.len() >= MAX_COPY_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= MAX_COPY_FILES {
            return;
        }
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let child_rel = rel_dir.join(&name);
        if ft.is_symlink() {
            let Ok(target) = std::fs::canonicalize(entry.path()) else {
                continue;
            };
            if !target.starts_with(canon_project) {
                warnings.push(format!(
                    "Skipped symlink outside project: \"{}\"",
                    child_rel.display()
                ));
                continue;
            }
            if target.is_file() && seen.insert(child_rel.clone()) {
                out.push(child_rel);
            }
        } else if ft.is_dir() {
            collect_dir_files(
                canon_project,
                &entry.path(),
                &child_rel,
                depth + 1,
                seen,
                out,
                warnings,
            );
        } else if ft.is_file() && seen.insert(child_rel.clone()) {
            out.push(child_rel);
        }
    }
}

fn walk_for_globs(
    canon_project: &Path,
    globs: &[String],
    include_node_modules: bool,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) {
    let mut stack: Vec<(PathBuf, PathBuf, usize)> =
        vec![(canon_project.to_path_buf(), PathBuf::new(), 0)];
    let mut visited = 0usize;
    while let Some((dir, rel_dir, depth)) = stack.pop() {
        if depth > MAX_WALK_DEPTH || visited >= MAX_WALK_ENTRIES || out.len() >= MAX_COPY_FILES {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            visited += 1;
            if visited > MAX_WALK_ENTRIES || out.len() >= MAX_COPY_FILES {
                return;
            }
            let name = entry.file_name();
            if name == ".git" || (!include_node_modules && name == "node_modules") {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            let child_rel = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(&name)
            } else {
                rel_dir.join(&name)
            };
            if ft.is_dir() {
                stack.push((entry.path(), child_rel, depth + 1));
            } else if ft.is_file() || ft.is_symlink() {
                let rel_str = child_rel.to_string_lossy().replace('\\', "/");
                if globs.iter().any(|g| glob_match(g, &rel_str)) && seen.insert(child_rel.clone()) {
                    out.push(child_rel);
                }
            }
        }
    }
}

// ---- command execution -----------------------------------------------------

async fn read_capped<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut total = 0usize;
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                total = total.saturating_add(n);
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > MAX_STREAM_CAPTURE_BYTES {
                    let drop = buf.len() - MAX_STREAM_CAPTURE_BYTES;
                    buf.drain(..drop);
                }
                if total > MAX_STREAM_CAPTURE_BYTES * 8 {
                    // Drain remaining without buffering indefinitely.
                    while matches!(reader.read(&mut chunk).await, Ok(m) if m > 0) {}
                    break;
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn trim_tail_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let skip = count - max_chars;
    let tail: String = s.chars().skip(skip).collect();
    match tail.find('\n') {
        Some(nl) => format!("…\n{}", &tail[nl + 1..]),
        None => format!("…{tail}"),
    }
}

/// Runs a worktree setup or teardown shell command inside `worktree_path` with a bounded
/// timeout and redacted output.
pub async fn run_hook_command(
    command: &str,
    worktree_path: &Path,
    project_path: &Path,
    branch: Option<&str>,
    agent_id: &str,
    timeout: Duration,
) -> HookRunOutcome {
    let mut cmd = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command]);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    crate::childenv::sanitize_child_env(&mut cmd);
    cmd.current_dir(worktree_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("KVISTCODE_WORKTREE_PATH", worktree_path)
        .env("KVISTCODE_PROJECT_PATH", project_path)
        .env("KVISTCODE_AGENT_ID", agent_id)
        .env("ROOT_WORKTREE_PATH", project_path)
        .env("CONDUCTOR_ROOT_PATH", project_path)
        .env("CONDUCTOR_WORKSPACE_PATH", worktree_path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1");
    if let Some(b) = branch {
        cmd.env("KVISTCODE_BRANCH", b);
    }
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: don't flash console windows from a GUI app.
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return HookRunOutcome {
                success: false,
                error: Some(format!("Couldn't start command: {e}")),
                output: String::new(),
            };
        }
    };

    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_task = tokio::spawn(async move {
        match stdout {
            Some(r) => read_capped(r).await,
            None => String::new(),
        }
    });
    let err_task = tokio::spawn(async move {
        match stderr {
            Some(r) => read_capped(r).await,
            None => String::new(),
        }
    });

    let waited = tokio::time::timeout(timeout, child.wait()).await;
    let (success, error) = match waited {
        Ok(Ok(status)) if status.success() => (true, None),
        Ok(Ok(status)) => {
            let msg = match status.code() {
                Some(code) => format!("Exited with status {code}"),
                None => "Terminated by signal".into(),
            };
            (false, Some(msg))
        }
        Ok(Err(e)) => (false, Some(format!("Wait failed: {e}"))),
        Err(_) => {
            kill_tree(&mut child, pid).await;
            let secs = timeout.as_secs();
            let msg = if secs >= 60 && secs.is_multiple_of(60) {
                format!("Timed out after {} minute(s)", secs / 60)
            } else {
                format!("Timed out after {secs} second(s)")
            };
            (false, Some(msg))
        }
    };

    let (out_s, err_s) = tokio::join!(
        async {
            tokio::time::timeout(Duration::from_secs(2), out_task)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default()
        },
        async {
            tokio::time::timeout(Duration::from_secs(2), err_task)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default()
        }
    );

    let combined = match (out_s.trim(), err_s.trim()) {
        ("", "") => String::new(),
        (o, "") => o.to_string(),
        ("", e) => e.to_string(),
        (o, e) => format!("{o}\n{e}"),
    };
    let redacted = redact_secrets(&combined);
    let output = trim_tail_chars(&redacted, MAX_SURFACED_OUTPUT_CHARS);

    HookRunOutcome {
        success,
        error,
        output,
    }
}

async fn kill_tree(child: &mut tokio::process::Child, pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(p) = pid {
        let pgid = -(p as libc::pid_t);
        unsafe {
            libc::kill(pgid, libc::SIGTERM);
        }
        if tokio::time::timeout(Duration::from_millis(400), child.wait())
            .await
            .is_ok()
        {
            return;
        }
        unsafe {
            libc::kill(pgid, libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = pid;
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kvistcode-wt-hooks-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn glob_matches_literals_wildcards_and_doublestar() {
        assert!(glob_match(".env", ".env"));
        assert!(glob_match(".env.*", ".env.local"));
        assert!(!glob_match(".env.*", "sub/.env.local"));
        assert!(glob_match("**/.env*", ".env"));
        assert!(glob_match("**/.env*", "apps/web/.env.local"));
        assert!(glob_match("config/*.local.json", "config/dev.local.json"));
        assert!(!glob_match(
            "config/*.local.json",
            "config/a/dev.local.json"
        ));
        assert!(glob_match("a/?/c.txt", "a/x/c.txt"));
        assert!(!glob_match("a/?/c.txt", "a/xy/c.txt"));
    }

    #[test]
    fn repo_config_priority_and_formats() {
        let dir = scratch("priority");
        // 1. conductor.json alone
        std::fs::write(
            dir.join("conductor.json"),
            r#"{"scripts": {"setup": "npm ci", "archive": "rm -rf tmp"}}"#,
        )
        .unwrap();
        let r = detect_repo_config(&dir).unwrap();
        assert_eq!(r.source, WorktreeHookSource::ConductorJson);
        assert_eq!(r.setup_command.as_deref(), Some("npm ci"));
        assert_eq!(r.teardown_command.as_deref(), Some("rm -rf tmp"));
        assert!(r.script_hash.is_some());
        assert!(!r.trusted);

        // 2. .cursor/worktrees.json beats conductor.json
        std::fs::create_dir_all(dir.join(".cursor")).unwrap();
        std::fs::write(
            dir.join(".cursor/worktrees.json"),
            r#"{"setup-worktree": ["cp $ROOT_WORKTREE_PATH/.env .env", "pnpm install"]}"#,
        )
        .unwrap();
        let r = detect_repo_config(&dir).unwrap();
        assert_eq!(r.source, WorktreeHookSource::CursorJson);
        assert_eq!(
            r.setup_command.as_deref(),
            Some("cp $ROOT_WORKTREE_PATH/.env .env && pnpm install")
        );

        // 3. .kvistcode.json beats both
        std::fs::write(
            dir.join(".kvistcode.json"),
            r#"{"worktree": {"copyFiles": [".env", "config/*.local.json"], "setupCommand": "cargo check", "teardownCommand": "echo bye"}}"#,
        )
        .unwrap();
        let r = detect_repo_config(&dir).unwrap();
        assert_eq!(r.source, WorktreeHookSource::KvistcodeJson);
        assert_eq!(r.copy_files, vec![".env", "config/*.local.json"]);
        assert_eq!(r.setup_command.as_deref(), Some("cargo check"));
        assert_eq!(r.teardown_command.as_deref(), Some("echo bye"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trust_hash_invalidates_when_command_changes() {
        let h1 = compute_script_hash(".kvistcode.json", Some("npm ci"), None).unwrap();
        let h2 = compute_script_hash(".kvistcode.json", Some("npm ci && curl evil"), None).unwrap();
        assert_ne!(h1, h2);
        assert_eq!(compute_script_hash(".kvistcode.json", None, None), None);
    }

    #[test]
    fn resolve_merges_settings_and_enforces_trust() {
        let dir = scratch("resolve");
        std::fs::write(
            dir.join(".kvistcode.json"),
            r#"{"worktree": {"copyFiles": [".env"], "setupCommand": "npm ci"}}"#,
        )
        .unwrap();
        let key = dir.to_string_lossy().into_owned();
        let mut settings = Settings::default();

        // Untrusted repo command -> needs_trust = true.
        let r = resolve(&key, &settings);
        assert!(r.setup_from_repo);
        assert!(r.needs_trust);
        assert_eq!(r.copy_files, vec![".env"]);

        // Trusting the exact hash clears needs_trust.
        let hash = r.repo.as_ref().unwrap().script_hash.clone().unwrap();
        settings.trusted_worktree_hooks.insert(key.clone(), hash);
        let r2 = resolve(&key, &settings);
        assert!(!r2.needs_trust);
        assert!(r2.repo.as_ref().unwrap().trusted);

        // User settings override setupCommand -> no trust needed even if repo is untrusted.
        settings.trusted_worktree_hooks.clear();
        settings.project_worktrees.insert(
            key.clone(),
            ProjectWorktreeConfig {
                copy_files: vec![".env.local".into()],
                setup_command: Some("pnpm i".into()),
                teardown_command: None,
            },
        );
        let r3 = resolve(&key, &settings);
        assert!(!r3.setup_from_repo);
        assert!(!r3.needs_trust);
        assert_eq!(r3.setup_command.as_deref(), Some("pnpm i"));
        assert_eq!(r3.copy_files, vec![".env", ".env.local"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_files_copies_safely_and_rejects_traversal_and_existing() {
        let proj = scratch("copy-proj");
        let wt = scratch("copy-wt");
        std::fs::write(proj.join(".env"), "SECRET=1\n").unwrap();
        std::fs::write(proj.join(".env.local"), "LOCAL=2\n").unwrap();
        std::fs::create_dir_all(proj.join("config")).unwrap();
        std::fs::write(proj.join("config/dev.local.json"), "{}").unwrap();
        std::fs::write(proj.join("tracked.txt"), "from-main").unwrap();
        std::fs::write(wt.join("tracked.txt"), "from-worktree").unwrap();

        let summary = copy_files(
            &proj,
            &wt,
            &[
                ".env".into(),
                "config/*.local.json".into(),
                "tracked.txt".into(),
                "../escape.txt".into(),
                ".git/config".into(),
            ],
        );

        assert_eq!(summary.copied, vec![".env", "config/dev.local.json"]);
        assert_eq!(summary.skipped, 1);
        assert_eq!(summary.warnings.len(), 2);
        assert_eq!(
            std::fs::read_to_string(wt.join(".env")).unwrap(),
            "SECRET=1\n"
        );
        assert_eq!(
            std::fs::read_to_string(wt.join("tracked.txt")).unwrap(),
            "from-worktree"
        );
        assert!(!wt.join(".env.local").exists());

        let _ = std::fs::remove_dir_all(&proj);
        let _ = std::fs::remove_dir_all(&wt);
    }

    #[cfg(unix)]
    #[test]
    fn copy_files_rejects_symlinks_escaping_project() {
        use std::os::unix::fs::symlink;
        let outside = scratch("outside");
        let proj = scratch("sym-proj");
        let wt = scratch("sym-wt");
        let secret = outside.join("secret.txt");
        std::fs::write(&secret, "top-secret").unwrap();
        symlink(&secret, proj.join(".env")).unwrap();
        symlink(&secret, proj.join(".kvistcode.json")).unwrap();

        let repo = detect_repo_config(&proj).unwrap();
        assert!(repo.parse_error.is_some());

        let summary = copy_files(&proj, &wt, &[".env".into()]);
        assert!(summary.copied.is_empty());
        assert!(!wt.join(".env").exists());
        assert!(!summary.warnings.is_empty());

        let _ = std::fs::remove_dir_all(&outside);
        let _ = std::fs::remove_dir_all(&proj);
        let _ = std::fs::remove_dir_all(&wt);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_hook_command_sets_env_redacts_secrets_and_times_out() {
        let proj = scratch("cmd-proj");
        let wt = scratch("cmd-wt");
        let out = run_hook_command(
            "echo \"wt=$KVISTCODE_WORKTREE_PATH root=$ROOT_WORKTREE_PATH key=sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd\"",
            &wt,
            &proj,
            Some("kvistcode/test"),
            "agent-1",
            Duration::from_secs(10),
        )
        .await;
        assert!(out.success, "{out:?}");
        assert!(out.output.contains(&wt.to_string_lossy().to_string()));
        assert!(out.output.contains(&proj.to_string_lossy().to_string()));
        assert!(!out
            .output
            .contains("sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ"));

        let timed = run_hook_command(
            "sleep 10",
            &wt,
            &proj,
            None,
            "agent-1",
            Duration::from_millis(150),
        )
        .await;
        assert!(!timed.success);
        assert!(timed.error.as_deref().unwrap_or("").contains("Timed out"));

        let _ = std::fs::remove_dir_all(&proj);
        let _ = std::fs::remove_dir_all(&wt);
    }
}
