import { describe, expect, it } from "vitest";
import { groupActivity, replyThoughts } from "./activity";
import { buildRows } from "./rows";
import type { TimelineItem } from "./types";

let n = 0;
const base = () => ({ id: `r${n++}`, ts: 0 });
const tool = () =>
  ({ ...base(), kind: "tool_call", toolCallId: `t${n}`, title: "x", toolKind: "read", status: "completed", locations: [], diffs: [], output: "", command: null }) as unknown as TimelineItem;
const user = (midTurn = false) => ({ ...base(), kind: "user_message", text: "hi", midTurn }) as unknown as TimelineItem;
const msg = () => ({ ...base(), kind: "agent_message", text: "ok" }) as unknown as TimelineItem;
const thought = () => ({ ...base(), kind: "thought", text: "Textures come later." }) as unknown as TimelineItem;
const all = (items: TimelineItem[]) => items.map((_, i) => i);

describe("replyThoughts on the phone (same rule as the desktop)", () => {
  it("is the first thought after a message sent while the agent worked, unless it answered in a message", () => {
    const items = [user(), tool(), user(true), tool(), thought(), tool(), thought()];
    expect([...replyThoughts(items, all(items))]).toEqual([4]);
    const answered = [user(), user(true), msg(), thought()];
    expect(replyThoughts(answered, all(answered)).size).toBe(0);
  });

  it("is its own row, marked as a reply, between the grouped steps", () => {
    const items = [user(), tool(), user(true), tool(), tool(), thought(), tool(), thought(), tool()];
    expect(groupActivity(items, all(items)).rows).toEqual([0, 1, 2, 3, 5, 6]);
    const rows = buildRows(items);
    const reply = rows.find((r) => r.key === items[5].id);
    expect(reply).toMatchObject({ kind: "item", reply: true });
    expect(rows.filter((r) => r.kind === "item" && r.reply)).toHaveLength(1);
  });
});
