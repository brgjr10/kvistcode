"""Codex app-server stand-in that lists skills and answers `thread/compact/start`.
Its final message of each turn reports what the turn carried."""
import json, sys

def send(m):
    sys.stdout.write(json.dumps(m) + "\n"); sys.stdout.flush()

def note(method, params):
    send({"jsonrpc": "2.0", "method": method, "params": params})

n = 0
def run_turn(mid, text):
    global n
    n += 1
    tid = "t%d" % n
    send({"jsonrpc": "2.0", "id": mid, "result": {"turn": {"id": tid}}})
    note("turn/started", {"threadId": "th1", "turn": {"id": tid}})
    note("item/completed", {"threadId": "th1", "item": {"id": "m%d" % n, "type": "agentMessage", "text": text}})
    note("turn/completed", {"threadId": "th1", "turn": {"id": tid, "status": "completed"}})

while True:
    line = sys.stdin.readline()
    if not line:
        sys.exit(0)
    msg = json.loads(line); method = msg.get("method"); mid = msg.get("id")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
    elif method == "thread/start":
        send({"jsonrpc": "2.0", "id": mid, "result": {"thread": {"id": "th1"}, "model": "m"}})
    elif method == "model/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"data": []}})
    elif method == "skills/list":
        send({"jsonrpc": "2.0", "id": mid, "result": {"data": [{"cwd": "/", "errors": [], "skills": [
            {"name": "deploy", "path": "/skills/deploy/SKILL.md", "enabled": True,
             "description": "Deploys", "shortDescription": "Ship it", "scope": "user"}]}]}})
    elif method == "turn/start":
        items = msg["params"]["input"]
        kinds = ",".join(i["type"] for i in items)
        run_turn(mid, "input=%s text=%s" % (kinds, items[0]["text"]))
    elif method == "thread/compact/start":
        # Codex runs a compaction as a turn of its own.
        run_turn(mid, "compacted")
    elif mid is not None and method is not None:
        send({"jsonrpc": "2.0", "id": mid, "result": {}})
