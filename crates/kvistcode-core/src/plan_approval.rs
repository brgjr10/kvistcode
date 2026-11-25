//! Plan approvals: the permission request claude-agent-acp sends when the agent
//! in plan mode calls `ExitPlanMode`. It is an ordinary `session/request_permission`
//! whose tool call has `kind: "switch_mode"` and the plan markdown in `rawInput.plan`:
//!
//! ```json
//! { "toolCall": { "toolCallId": "toolu_…", "name": "ExitPlanMode", "title": "Ready to code?",
//!                 "kind": "switch_mode", "rawInput": { "plan": "# Plan\n…" } },
//!   "options": [ { "optionId": "exit-plan-auto", "name": "Yes, and use auto mode", "kind": "allow_always" },
//!                { "optionId": "exit-plan-default", "name": "Yes, manually approve edits", "kind": "allow_once" },
//!                { "optionId": "reject", "name": "No, keep planning", "kind": "reject_once" } ] }
//! ```
//!
//! The options vary with the session's modes (also `exit-plan-accept-edits`,
//! `exit-plan-bypass` and `exit-plan-clear-*`); the UI maps them by id.

use serde_json::Value;

/// Plans longer than this are cut; a plan is a page or two, not a log.
const MAX_PLAN_BYTES: usize = 200 * 1024;

/// Tool names that ask the user to approve a plan before coding.
const PLAN_APPROVAL_TOOLS: &[&str] = &[
    "exitplanmode",
    "exit_plan_mode",
    "approve_plan",
    "approveplan",
    "request_plan_approval",
    "plan_approval",
    // OpenCode's switch from its plan agent to its build agent.
    "plan_exit",
];

pub fn is_plan_approval_call(tool_call: &Value) -> bool {
    let kind = tool_call["kind"].as_str();
    if kind == Some("switch_mode") {
        return true;
    }
    if !matches!(kind, None | Some("" | "other" | "think")) {
        return false;
    }
    let name = tool_call["_meta"]["claudeCode"]["toolName"]
        .as_str()
        .or_else(|| tool_call["_meta"]["toolName"].as_str())
        .or_else(|| tool_call["name"].as_str())
        .or_else(|| tool_call["toolName"].as_str());
    name.is_some_and(|n| PLAN_APPROVAL_TOOLS.contains(&n.trim().to_ascii_lowercase().as_str()))
}

/// The plan of a plan-approval request, or `None` for any other permission
/// request. Strict on purpose: the tool must be a mode switch or plan-approval
/// tool *and* carry plan text.
pub fn plan_from_tool_call(tool_call: &Value) -> Option<String> {
    if !is_plan_approval_call(tool_call) {
        return None;
    }
    let raw = &tool_call["rawInput"];
    let plan = raw["plan"]
        .as_str()
        .or_else(|| raw["planMarkdown"].as_str())
        .or_else(|| raw["plan_markdown"].as_str())?
        .trim();
    if plan.is_empty() {
        return None;
    }
    if plan.len() <= MAX_PLAN_BYTES {
        return Some(plan.to_string());
    }
    let mut end = MAX_PLAN_BYTES;
    while !plan.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}\n\n_… plan truncated_", &plan[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(kind: &str, raw: Value) -> Value {
        json!({
            "toolCallId": "toolu_1", "name": "ExitPlanMode", "title": "Ready to code?",
            "kind": kind, "rawInput": raw, "content": [],
        })
    }

    #[test]
    fn extracts_the_plan_of_an_exit_plan_mode_request() {
        let tc = call("switch_mode", json!({ "plan": "\n# Plan\n- one\n- two\n" }));
        assert_eq!(
            plan_from_tool_call(&tc).as_deref(),
            Some("# Plan\n- one\n- two")
        );
    }

    #[test]
    fn ignores_everything_else() {
        // Entering plan mode has the same kind but no plan.
        assert_eq!(plan_from_tool_call(&call("switch_mode", json!({}))), None);
        assert_eq!(
            plan_from_tool_call(&call("switch_mode", json!({ "plan": "  " }))),
            None
        );
        assert_eq!(
            plan_from_tool_call(&call("switch_mode", json!({ "plan": 3 }))),
            None
        );
        // A plan field on some other tool is not an approval of a plan.
        assert_eq!(
            plan_from_tool_call(&call("execute", json!({ "plan": "x" }))),
            None
        );
        assert_eq!(plan_from_tool_call(&json!({})), None);
    }

    #[test]
    fn extracts_plan_from_other_providers_plan_approval_tools() {
        let tc = json!({
            "toolCallId": "tc_2",
            "name": "exit_plan_mode",
            "kind": "other",
            "rawInput": { "planMarkdown": "# Gemini plan\n- step 1" },
        });
        assert_eq!(
            plan_from_tool_call(&tc).as_deref(),
            Some("# Gemini plan\n- step 1")
        );
    }

    #[test]
    fn caps_the_plan_on_a_char_boundary() {
        let plan = "é".repeat(MAX_PLAN_BYTES);
        let out = plan_from_tool_call(&call("switch_mode", json!({ "plan": plan }))).unwrap();
        assert!(out.len() <= MAX_PLAN_BYTES + 40);
        assert!(out.ends_with("_… plan truncated_"));
    }
}
