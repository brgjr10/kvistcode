//! Sessions started in a plain terminal (`claude`, `codex`), found on disk so they can be
//! imported. Read-only: nothing under `~/.claude` or `~/.codex` is ever written. The home
//! folder is a parameter, so tests use a scratch folder. Transcript text is returned to the
//! caller and never logged.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Provider;

/// The most of one session file that is read; a bigger file is read up to here.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// The longest single line that is parsed (a huge tool output is skipped).
const MAX_LINE_BYTES: usize = 2 * 1024 * 1024;
/// Session files looked at per provider.
const MAX_FILES: usize = 3000;
/// Sessions returned to the picker.
const MAX_SESSIONS: usize = 200;
/// Messages kept when a transcript is imported (the latest ones).
pub const MAX_IMPORT_MESSAGES: usize = 200;
/// Longest text kept per imported message, in characters.
pub const MAX_MESSAGE_CHARS: usize = 20_000;
const TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSession {
    pub provider: Provider,
    /// The provider's own session id (what `claude --resume` / `codex resume` take).
    pub id: String,
    pub cwd: String,
    /// The first thing the user said.
    pub title: String,
    /// When the session started (unix ms), from the file when it says, else its mtime.
    pub started_at: i64,
    pub messages: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub provider: Provider,
    pub account_id: String,
    pub project_path: String,
    pub session_id: String,
}

/// Whether Kvistcode can reopen a session of this provider by id and the terminal CLI can too.
pub fn supported(provider: Provider) -> bool {
    matches!(provider, Provider::Claude | Provider::Codex)
}

/// A provider session id is safe to put on a command line: letters, digits, `-` and `_`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Claude Code's folder name for a working directory: every non-alphanumeric becomes `-`.
fn claude_dir_name(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn same_dir(a: &str, b: &Path) -> bool {
    let norm =
        |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.components().collect::<PathBuf>());
    norm(Path::new(a)) == norm(b)
}

fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64)
}

fn parse_ts(v: &Value) -> Option<i64> {
    let s = v.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// Calls `f` with each parsable JSON line of `path` (read up to the size cap); malformed or
/// oversized lines are skipped.
fn for_each_line(path: &Path, mut f: impl FnMut(Value)) {
    let Ok(file) = File::open(path) else { return };
    let mut reader = BufReader::new(file.take(MAX_FILE_BYTES));
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if buf.len() > MAX_LINE_BYTES {
            continue;
        }
        if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
            f(v);
        }
    }
}

fn cap_text(s: &str) -> String {
    let t = s.trim();
    match t.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_string(),
    }
}

fn title_of(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or("");
    match line.char_indices().nth(TITLE_CHARS) {
        Some((i, _)) => format!("{}…", &line[..i]),
        None => line.to_string(),
    }
}

/// Text the CLI itself injects as a "user" message: not what the person said.
fn is_injected(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("<command-")
        || t.starts_with("<local-command")
        || t.starts_with("<environment_context>")
        || t.starts_with("<user_instructions>")
        || t.starts_with("<system-reminder>")
        || t.starts_with("# AGENTS.md")
        || t.starts_with("Caveat: The messages below were generated")
        || t.starts_with("[Request interrupted")
}

/// Text of a message body: a plain string, or the `text` / `input_text` / `output_text` blocks.
fn blocks_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter(|b| {
                matches!(
                    b["type"].as_str(),
                    Some("text" | "input_text" | "output_text")
                )
            })
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

// ---- Claude Code ----

/// One line of a Claude Code session file as a message, if it is one.
fn claude_message(v: &Value) -> Option<Message> {
    if v["isSidechain"].as_bool() == Some(true) || v["isMeta"].as_bool() == Some(true) {
        return None;
    }
    let role = match v["type"].as_str()? {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => return None,
    };
    let text = blocks_text(&v["message"]["content"]);
    if text.trim().is_empty() || (role == Role::User && is_injected(&text)) {
        return None;
    }
    Some(Message {
        role,
        text: cap_text(&text),
    })
}

fn scan_claude(path: &Path, project: &Path) -> Option<ExternalSession> {
    let id = path.file_stem()?.to_str()?.to_string();
    if !valid_id(&id) {
        return None;
    }
    let (mut cwd, mut title, mut started, mut messages) = (None::<String>, String::new(), None, 0);
    for_each_line(path, |v| {
        if cwd.is_none() {
            cwd = v["cwd"].as_str().map(str::to_string);
        }
        if started.is_none() {
            started = parse_ts(&v["timestamp"]);
        }
        if let Some(m) = claude_message(&v) {
            messages += 1;
            if title.is_empty() && m.role == Role::User {
                title = title_of(&m.text);
            }
        }
    });
    let cwd = cwd?;
    (messages > 0 && same_dir(&cwd, project)).then(|| ExternalSession {
        provider: Provider::Claude,
        id,
        cwd,
        title,
        started_at: started.unwrap_or_else(|| mtime_ms(path)),
        messages,
    })
}

fn claude_files(home: &Path, project: &Path) -> Vec<PathBuf> {
    let dir = home
        .join(".claude")
        .join("projects")
        .join(claude_dir_name(project));
    jsonl_in(&dir)
}

fn jsonl_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && p.is_file())
        .take(MAX_FILES)
        .collect()
}

// ---- Codex ----

fn codex_message(v: &Value) -> Option<Message> {
    if v["type"].as_str()? != "response_item" {
        return None;
    }
    let p = &v["payload"];
    if p["type"].as_str()? != "message" {
        return None;
    }
    let role = match p["role"].as_str()? {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => return None,
    };
    let text = blocks_text(&p["content"]);
    if text.trim().is_empty() || (role == Role::User && is_injected(&text)) {
        return None;
    }
    Some(Message {
        role,
        text: cap_text(&text),
    })
}

fn scan_codex(path: &Path, project: &Path) -> Option<ExternalSession> {
    let (mut meta, mut title, mut messages) =
        (None::<(String, String, Option<i64>)>, String::new(), 0);
    for_each_line(path, |v| {
        if meta.is_none() && v["type"].as_str() == Some("session_meta") {
            let p = &v["payload"];
            if let (Some(id), Some(cwd)) = (p["id"].as_str(), p["cwd"].as_str()) {
                let ts = parse_ts(&p["timestamp"]).or_else(|| parse_ts(&v["timestamp"]));
                meta = Some((id.to_string(), cwd.to_string(), ts));
            }
        }
        if let Some(m) = codex_message(&v) {
            messages += 1;
            if title.is_empty() && m.role == Role::User {
                title = title_of(&m.text);
            }
        }
    });
    let (id, cwd, ts) = meta?;
    (valid_id(&id) && messages > 0 && same_dir(&cwd, project)).then(|| ExternalSession {
        provider: Provider::Codex,
        id,
        cwd,
        title,
        started_at: ts.unwrap_or_else(|| mtime_ms(path)),
        messages,
    })
}

/// `rollout-*.jsonl` files under `~/.codex/sessions` (a year/month/day tree), newest folders
/// first so the file cap drops the oldest.
fn codex_files(home: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        entries.reverse();
        for p in entries {
            if out.len() >= MAX_FILES {
                return;
            }
            if p.is_dir() {
                if depth < 4 {
                    walk(&p, depth + 1, out);
                }
            } else if p.is_file()
                && p.extension().is_some_and(|x| x == "jsonl")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("rollout-"))
            {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&home.join(".codex").join("sessions"), 0, &mut out);
    out
}

// ---- public API ----

/// Sessions of `provider` that were run in `project`, newest first.
pub fn list_sessions(home: &Path, provider: Provider, project: &Path) -> Vec<ExternalSession> {
    let mut out: Vec<ExternalSession> = match provider {
        Provider::Claude => claude_files(home, project)
            .iter()
            .filter_map(|p| scan_claude(p, project))
            .collect(),
        Provider::Codex => codex_files(home)
            .iter()
            .filter_map(|p| scan_codex(p, project))
            .collect(),
        _ => Vec::new(),
    };
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(a.id.cmp(&b.id)));
    out.truncate(MAX_SESSIONS);
    out
}

/// The latest messages of one session (at most [`MAX_IMPORT_MESSAGES`]), oldest first.
pub fn read_transcript(home: &Path, provider: Provider, project: &Path, id: &str) -> Vec<Message> {
    if !valid_id(id) {
        return Vec::new();
    }
    let path = match provider {
        Provider::Claude => claude_files(home, project)
            .into_iter()
            .find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(id)),
        Provider::Codex => codex_files(home).into_iter().find(|p| {
            let mut found = false;
            for_each_first_meta(p, |sid| found = sid == id);
            found
        }),
        _ => None,
    };
    let Some(path) = path else {
        return Vec::new();
    };
    let mut msgs = Vec::new();
    for_each_line(&path, |v| {
        let m = match provider {
            Provider::Claude => claude_message(&v),
            _ => codex_message(&v),
        };
        msgs.extend(m);
    });
    let skip = msgs.len().saturating_sub(MAX_IMPORT_MESSAGES);
    msgs.drain(..skip);
    msgs
}

/// Reads only as far as a Codex file's `session_meta` and hands over its id.
fn for_each_first_meta(path: &Path, mut f: impl FnMut(&str)) {
    let Ok(file) = File::open(path) else { return };
    let mut line = Vec::new();
    // The meta line is the first; a few lines of slack for odd files.
    let mut reader = BufReader::new(file.take(MAX_LINE_BYTES as u64 * 2));
    for _ in 0..5 {
        line.clear();
        if !matches!(reader.read_until(b'\n', &mut line), Ok(n) if n > 0) {
            return;
        }
        if let Ok(v) = serde_json::from_slice::<Value>(&line) {
            if v["type"].as_str() == Some("session_meta") {
                if let Some(id) = v["payload"]["id"].as_str() {
                    f(id);
                }
                return;
            }
        }
    }
}

/// The command that reopens `session_id` in a terminal, or None when Kvistcode can't say.
pub fn terminal_command(provider: Provider, session_id: &str) -> Option<String> {
    if !valid_id(session_id) {
        return None;
    }
    match provider {
        Provider::Claude => Some(format!("claude --resume {session_id}")),
        Provider::Codex => Some(format!("codex resume {session_id}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kvist-ext-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn claude_fixture(home: &Path, project: &Path, id: &str, body: &str) {
        let dir = home.join(".claude/projects").join(claude_dir_name(project));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{id}.jsonl")), body).unwrap();
    }

    fn json_str(s: &str) -> String {
        let q = serde_json::to_string(s).unwrap();
        q[1..q.len() - 1].to_string()
    }

    #[test]
    fn claude_sessions_are_listed_with_title_and_count() {
        let home = scratch("c1");
        let project = home.join("work/app");
        fs::create_dir_all(&project).unwrap();
        // JSON-escaped, so Windows backslashes survive.
        let cwd = json_str(&project.to_string_lossy());
        let body = format!(
            "{{\"type\":\"summary\",\"summary\":\"x\"}}\n\
             not json at all\n\
             {{\"type\":\"user\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-03-01T10:00:00Z\",\"message\":{{\"role\":\"user\",\"content\":\"<command-name>/init</command-name>\"}}}}\n\
             {{\"type\":\"user\",\"cwd\":\"{cwd}\",\"timestamp\":\"2026-03-01T10:00:01Z\",\"message\":{{\"role\":\"user\",\"content\":\"Fix the login bug\\nplease\"}}}}\n\
             {{\"type\":\"assistant\",\"cwd\":\"{cwd}\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"On it.\"}},{{\"type\":\"tool_use\",\"id\":\"t\"}}]}}}}\n\
             {{\"type\":\"user\",\"cwd\":\"{cwd}\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"tool_result\",\"content\":\"ok\"}}]}}}}\n\
             {{\"type\":\"assistant\",\"isSidechain\":true,\"message\":{{\"content\":\"sub\"}}}}\n"
        );
        claude_fixture(&home, &project, "abc-123", &body);
        // A session of another folder with the same encoded name is not listed.
        claude_fixture(
            &home,
            &project,
            "other",
            "{\"type\":\"user\",\"cwd\":\"/elsewhere\",\"message\":{\"content\":\"hi\"}}\n",
        );
        let list = list_sessions(&home, Provider::Claude, &project);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "abc-123");
        assert_eq!(list[0].title, "Fix the login bug");
        assert_eq!(list[0].messages, 2);
        assert_eq!(list[0].started_at, 1_772_359_200_000);
        let t = read_transcript(&home, Provider::Claude, &project, "abc-123");
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].role, Role::Assistant);
        assert_eq!(t[1].text, "On it.");
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn codex_sessions_are_found_by_cwd_and_sorted() {
        let home = scratch("x1");
        let project = home.join("proj");
        fs::create_dir_all(&project).unwrap();
        // JSON-escaped, so Windows backslashes survive.
        let cwd = json_str(&project.to_string_lossy());
        let dir = home.join(".codex/sessions/2026/03/02");
        fs::create_dir_all(&dir).unwrap();
        let mk = |id: &str, ts: &str, cwd: &str, first: &str| {
            format!(
                "{{\"timestamp\":\"{ts}\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"{cwd}\",\"timestamp\":\"{ts}\"}}}}\n\
                 garbage{{\n\
                 {{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"<environment_context>x</environment_context>\"}}]}}}}\n\
                 {{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{first}\"}}]}}}}\n\
                 {{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"done\"}}]}}}}\n\
                 {{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call\",\"name\":\"shell\"}}}}\n"
            )
        };
        fs::write(
            dir.join("rollout-a.jsonl"),
            mk("id-old", "2026-03-02T08:00:00Z", &cwd, "older task"),
        )
        .unwrap();
        fs::write(
            dir.join("rollout-b.jsonl"),
            mk("id-new", "2026-03-02T09:00:00Z", &cwd, "newer task"),
        )
        .unwrap();
        fs::write(
            dir.join("rollout-c.jsonl"),
            mk("id-x", "2026-03-02T10:00:00Z", "/not/here", "foreign"),
        )
        .unwrap();
        fs::write(dir.join("notes.jsonl"), "{}").unwrap();
        let list = list_sessions(&home, Provider::Codex, &project);
        assert_eq!(
            list.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["id-new", "id-old"]
        );
        assert_eq!(list[0].title, "newer task");
        assert_eq!(list[0].messages, 2);
        let t = read_transcript(&home, Provider::Codex, &project, "id-old");
        assert_eq!(t[0].text, "older task");
        assert!(read_transcript(&home, Provider::Codex, &project, "../etc").is_empty());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn missing_folders_and_other_providers_give_nothing() {
        let home = scratch("e1");
        let p = home.join("p");
        assert!(list_sessions(&home, Provider::Claude, &p).is_empty());
        assert!(list_sessions(&home, Provider::Codex, &p).is_empty());
        assert!(list_sessions(&home, Provider::Grok, &p).is_empty());
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn terminal_commands_need_a_safe_id() {
        assert_eq!(
            terminal_command(Provider::Claude, "a1-b_2").as_deref(),
            Some("claude --resume a1-b_2")
        );
        assert_eq!(
            terminal_command(Provider::Codex, "a1").as_deref(),
            Some("codex resume a1")
        );
        assert!(terminal_command(Provider::Claude, "a; rm -rf /").is_none());
        assert!(terminal_command(Provider::Grok, "a1").is_none());
    }

    #[test]
    fn long_text_is_capped_on_a_char_boundary() {
        let s = "é".repeat(MAX_MESSAGE_CHARS + 5);
        assert_eq!(cap_text(&s).chars().count(), MAX_MESSAGE_CHARS + 1);
        assert_eq!(title_of(&"é".repeat(200)).chars().count(), TITLE_CHARS + 1);
    }
}
