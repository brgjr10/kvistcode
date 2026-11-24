"""Mock ACP agent for auto-review integration tests.

Modes via env:
- MOCK_ROLE=worker: writes/updates a file in cwd on every prompt (unless prompt contains NO_EDIT),
  and logs prompts to MOCK_WORKER_LOG.
- MOCK_ROLE=reviewer: logs every review prompt & cwd to MOCK_REVIEW_LOG, tries requesting a
  permission (to verify Kvistcode denies it), and returns JSON findings controlled by
  MOCK_REVIEW_RESPONSE.
"""

import json
import os
import sys


def send(msg):
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


def update(sid, upd):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": upd}})


def read():
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    return json.loads(line)


def append_log(env_key, record):
    path = os.environ.get(env_key)
    if path:
        with open(path, "a", encoding="utf-8") as f:
            f.write(json.dumps(record) + "\n")


role = os.environ.get("MOCK_ROLE", "worker")
cwd = os.getcwd()
turn_n = 0

while True:
    msg = read()
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        send({
            "jsonrpc": "2.0",
            "id": mid,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {
                    "loadSession": True,
                    "sessionCapabilities": {"resume": {}, "systemPrompt": {"mode": {"append": {}}}},
                },
                "authMethods": [],
            },
        })
    elif method in ("session/new", "session/resume", "session/load"):
        cwd = msg["params"].get("cwd") or cwd
        sid = msg["params"].get("sessionId", f"sid-{role}")
        send({"jsonrpc": "2.0", "id": mid, "result": {"sessionId": sid}})
    elif method in ("session/set_config_option", "session/set_model", "session/set_mode"):
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "session/prompt":
        sid = msg["params"]["sessionId"]
        text = msg["params"]["prompt"][0]["text"]
        turn_n += 1
        if role == "worker":
            append_log("MOCK_WORKER_LOG", {"turn": turn_n, "text": text})
            if "NO_EDIT" not in text:
                target = os.path.join(cwd, "app.rs")
                with open(target, "w", encoding="utf-8") as f:
                    f.write(f"pub fn version() -> u32 {{ {turn_n} }}\n")
            update(sid, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": f"Updated app.rs (turn {turn_n})."}})
            send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
        else:
            # Verify permission requests are denied in read-only review mode.
            perm_id = 700 + turn_n
            send({
                "jsonrpc": "2.0",
                "id": perm_id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": sid,
                    "toolCall": {"toolCallId": "edit-attempt", "title": "Write file", "kind": "edit"},
                    "options": [
                        {"optionId": "allow", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "deny", "name": "Deny", "kind": "reject_once"},
                    ],
                },
            })
            while True:
                reply = read()
                if reply.get("id") == perm_id and "method" not in reply:
                    perm_outcome = reply.get("result", {}).get("outcome", {})
                    break
            append_log("MOCK_REVIEW_LOG", {"cwd": cwd, "permOutcome": perm_outcome, "prompt": text})
            reply_text = os.environ.get(
                "MOCK_REVIEW_RESPONSE",
                json.dumps({
                    "summary": "Found 1 critical bug in app.rs.",
                    "findings": [
                        {
                            "severity": "critical",
                            "path": "app.rs",
                            "line": 1,
                            "title": "Hardcoded version number",
                            "body": "Use CARGO_PKG_VERSION instead of a hardcoded integer.",
                        }
                    ],
                }),
            )
            update(sid, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": reply_text}})
            send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})
    elif mid is not None and method is not None:
        send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "unsupported"}})
