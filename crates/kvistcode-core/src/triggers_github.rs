//! Reading what GitHub says into trigger events. Pure: the desktop shell runs `gh` and
//! hands the JSON here, so all of it is tested against fixtures with no network.

use serde::Deserialize;

use crate::triggers::InboundEvent;

/// Failed checks on open pull requests, from
/// `gh pr list --json number,title,headRefName,headRefOid,url,statusCheckRollup`.
/// A pull request counts once its checks have all finished and at least one failed; the
/// event id includes the commit, so each push that is red fires once.
pub fn parse_ci_failures(json: &str) -> anyhow::Result<Vec<InboundEvent>> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Pr {
        number: u64,
        #[serde(default)]
        title: String,
        #[serde(default)]
        head_ref_name: String,
        #[serde(default)]
        head_ref_oid: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        status_check_rollup: Vec<Check>,
    }
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase", default)]
    struct Check {
        name: String,
        context: String,
        workflow_name: String,
        status: String,
        conclusion: String,
        state: String,
        details_url: String,
        target_url: String,
    }
    let prs: Vec<Pr> = serde_json::from_str(json)?;
    let mut out = Vec::new();
    for pr in prs {
        let mut failing = Vec::new();
        let mut pending = false;
        for c in &pr.status_check_rollup {
            if !c.state.is_empty() {
                // A commit status: SUCCESS, FAILURE, ERROR, PENDING, EXPECTED.
                match c.state.as_str() {
                    "FAILURE" | "ERROR" => failing.push((c.context.clone(), c.target_url.clone())),
                    "PENDING" | "EXPECTED" => pending = true,
                    _ => {}
                }
            } else if c.status != "COMPLETED" {
                pending = true;
            } else if matches!(
                c.conclusion.as_str(),
                "FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE"
            ) {
                let name = if c.workflow_name.is_empty() {
                    c.name.clone()
                } else {
                    format!("{} / {}", c.workflow_name, c.name)
                };
                failing.push((name, c.details_url.clone()));
            }
        }
        if pending || failing.is_empty() {
            continue;
        }
        let mut text = format!(
            "CI is failing on pull request #{} \"{}\" (branch {}, commit {}).\n{}\nFailing checks:\n",
            pr.number,
            pr.title,
            pr.head_ref_name,
            pr.head_ref_oid.chars().take(12).collect::<String>(),
            pr.url
        );
        for (name, url) in failing.iter().take(20) {
            text.push_str(&format!("- {name} {url}\n"));
        }
        out.push(InboundEvent {
            id: format!("ci:{}:{}", pr.number, pr.head_ref_oid),
            label: "GitHub CI failure".into(),
            text,
        });
    }
    Ok(out)
}

/// Which GitHub comment list a response came from (the ids of the two don't overlap, but
/// the event ids say so anyway).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    /// Pull request conversation and issue comments.
    Issue,
    /// Comments on a pull request's diff.
    Review,
}

/// People whose comments count unless the trigger allows outsiders.
fn is_trusted(association: &str) -> bool {
    matches!(association, "OWNER" | "MEMBER" | "COLLABORATOR")
}

/// Comments (from `gh api repos/{repo}/issues/comments` or `.../pulls/comments`) that
/// mention `keyword`, case-insensitively. Bots are skipped (so an agent's own comment
/// can't start another agent), and so are people without write access unless
/// `allow_outsiders`.
pub fn parse_mentions(
    json: &str,
    kind: CommentKind,
    keyword: &str,
    allow_outsiders: bool,
) -> anyhow::Result<Vec<InboundEvent>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct User {
        login: String,
        #[serde(rename = "type")]
        kind: String,
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Comment {
        id: u64,
        body: String,
        html_url: String,
        author_association: String,
        user: User,
    }
    let needle = keyword.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let comments: Vec<Comment> = serde_json::from_str(json)?;
    let prefix = match kind {
        CommentKind::Issue => "ic",
        CommentKind::Review => "rc",
    };
    Ok(comments
        .into_iter()
        .filter(|c| c.id != 0 && c.body.to_lowercase().contains(&needle))
        .filter(|c| !c.user.kind.eq_ignore_ascii_case("bot") && !c.user.login.ends_with("[bot]"))
        .filter(|c| allow_outsiders || is_trusted(&c.author_association))
        .map(|c| InboundEvent {
            id: format!("{prefix}:{}", c.id),
            label: "GitHub comment".into(),
            text: format!(
                "Comment by @{} ({}):\n\n{}",
                c.user.login, c.html_url, c.body
            ),
        })
        .collect())
}

/// `2026-11-24T10:00:00Z` for `ms` since the epoch (GitHub's `since` parameter).
pub fn iso_time(ms: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

/// How far back a mention poll looks: from the last poll (with an overlap for clock skew
/// and late indexing; seen ids make the overlap harmless), never before the trigger was
/// last edited.
pub fn mention_since(updated_at: i64, last_polled_at: Option<i64>) -> i64 {
    const OVERLAP_MS: i64 = 10 * 60 * 1000;
    match last_polled_at {
        Some(p) => (p - OVERLAP_MS).max(updated_at),
        None => updated_at,
    }
}

/// True if `text` (gh's stderr) says GitHub is rate limiting us, so the poller backs off.
pub fn is_rate_limited(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("rate limit")
        || t.contains("abuse")
        || t.contains("http 429")
        || t.contains("secondary")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRS: &str = r#"[
      {"number":7,"title":"Add thing","headRefName":"feat/x","headRefOid":"abcdef0123456789abcdef","url":"https://github.com/o/r/pull/7",
       "statusCheckRollup":[
         {"__typename":"CheckRun","name":"test","workflowName":"CI","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/o/r/actions/runs/1"},
         {"__typename":"CheckRun","name":"lint","workflowName":"CI","status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"u"}]},
      {"number":8,"title":"Still running","headRefName":"b","headRefOid":"1111","url":"u8",
       "statusCheckRollup":[
         {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"u"},
         {"__typename":"CheckRun","name":"slow","status":"IN_PROGRESS","conclusion":""}]},
      {"number":9,"title":"Green","headRefName":"g","headRefOid":"2222","url":"u9",
       "statusCheckRollup":[{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"SUCCESS"}]},
      {"number":10,"title":"Legacy status","headRefName":"l","headRefOid":"3333","url":"u10",
       "statusCheckRollup":[{"__typename":"StatusContext","context":"ci/legacy","state":"ERROR","targetUrl":"https://ex.com/1"}]},
      {"number":11,"title":"No checks","headRefName":"n","headRefOid":"4444","url":"u11","statusCheckRollup":[]}
    ]"#;

    #[test]
    fn only_finished_failing_pull_requests_are_events() {
        let events = parse_ci_failures(PRS).unwrap();
        let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["ci:7:abcdef0123456789abcdef", "ci:10:3333"]);
        assert!(events[0]
            .text
            .contains("- CI / test https://github.com/o/r/actions/runs/1"));
        assert!(!events[0].text.contains("lint"));
        assert!(events[0].text.contains("commit abcdef012345)"));
        assert!(events[1].text.contains("- ci/legacy https://ex.com/1"));
        assert!(parse_ci_failures("[]").unwrap().is_empty());
        assert!(parse_ci_failures("not json").is_err());
    }

    const COMMENTS: &str = r#"[
      {"id":1,"body":"Hey @Kvistcode please look","html_url":"https://github.com/o/r/issues/3#issuecomment-1","author_association":"MEMBER","user":{"login":"alice","type":"User"}},
      {"id":2,"body":"@kvistcode from a stranger","html_url":"h2","author_association":"NONE","user":{"login":"mallory","type":"User"}},
      {"id":3,"body":"@kvistcode (bot reply)","html_url":"h3","author_association":"MEMBER","user":{"login":"kv[bot]","type":"Bot"}},
      {"id":4,"body":"no mention here","html_url":"h4","author_association":"OWNER","user":{"login":"bob","type":"User"}},
      {"id":5,"body":"@kvistcode owner","html_url":"h5","author_association":"OWNER","user":{"login":"carol","type":"User"}}
    ]"#;

    #[test]
    fn mentions_skip_bots_and_outsiders_by_default() {
        let ev = parse_mentions(COMMENTS, CommentKind::Issue, "@kvistcode", false).unwrap();
        let ids: Vec<&str> = ev.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["ic:1", "ic:5"]);
        assert!(ev[0]
            .text
            .starts_with("Comment by @alice (https://github.com/o/r/issues/3#issuecomment-1)"));
        let all = parse_mentions(COMMENTS, CommentKind::Review, " @KVISTCODE ", true).unwrap();
        let ids: Vec<&str> = all.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["rc:1", "rc:2", "rc:5"], "bots are still skipped");
        assert!(parse_mentions(COMMENTS, CommentKind::Issue, "  ", true)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn since_never_reaches_before_the_last_edit() {
        assert_eq!(mention_since(1_000_000, None), 1_000_000);
        assert_eq!(
            mention_since(1_000_000, Some(2_000_000)),
            2_000_000 - 600_000
        );
        assert_eq!(mention_since(5_000_000, Some(2_000_000)), 5_000_000);
        assert_eq!(iso_time(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn rate_limit_messages_are_recognised() {
        assert!(is_rate_limited(
            "gh: API rate limit exceeded for user (HTTP 403)"
        ));
        assert!(is_rate_limited("You have exceeded a secondary rate limit"));
        assert!(!is_rate_limited("gh: Not Found (HTTP 404)"));
    }
}
