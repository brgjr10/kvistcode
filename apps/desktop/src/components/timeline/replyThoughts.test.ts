import { describe, expect, it } from "vitest";
import type { TimelineItem } from "@/lib/types";
import { groupActivity, replyThoughts } from "./activityGroups";
import { turnFolds } from "./turnFold";

let n = 0;
const base = () => ({ id: `r${n++}`, ts: 0 });
const tool = () =>
  ({ ...base(), kind: "tool_call", toolCallId: `t${n}`, title: "x", toolKind: "read", status: "completed", locations: [], diffs: [], output: "", command: null }) as unknown as TimelineItem;
const user = (midTurn = false) => ({ ...base(), kind: "user_message", text: "hi", midTurn }) as unknown as TimelineItem;
const msg = () => ({ ...base(), kind: "agent_message", text: "ok" }) as unknown as TimelineItem;
const thought = () => ({ ...base(), kind: "thought", text: "Textures come later." }) as unknown as TimelineItem;
const all = (items: TimelineItem[]) => items.map((_, i) => i);

describe("replyThoughts (an answer given only in thinking)", () => {
  it("is the first thought after a message sent while the agent worked", () => {
    const items = [user(), tool(), user(true), tool(), tool(), thought(), tool(), thought(), tool()];
    expect([...replyThoughts(items, all(items))]).toEqual([5]);
  });

  it("is none when the agent answered in a message, or the message started a turn", () => {
    const answered = [user(), tool(), user(true), msg(), thought(), tool()];
    expect(replyThoughts(answered, all(answered)).size).toBe(0);
    const fresh = [user(), thought(), tool(), tool()];
    expect(replyThoughts(fresh, all(fresh)).size).toBe(0);
  });

  it("stays out of the step groups, and the steps around it still group", () => {
    const items = [user(), tool(), user(true), tool(), tool(), thought(), tool(), thought(), tool()];
    const g = groupActivity(items, all(items));
    expect(g.rows).toEqual([0, 1, 2, 3, 5, 6]);
    expect(g.groups.get(3)).toEqual([3, 4]);
    expect(g.groups.get(6)).toEqual([6, 7, 8]);
  });

  it("stays out of Minimal's folded steps", () => {
    const items = [user(), tool(), user(true), tool(), thought(), tool(), msg()];
    const finals = new Set([6]);
    const folded = turnFolds(items, all(items), finals).flatMap((f) => f.rows);
    expect(folded).toEqual([1, 3, 5]);
    expect(folded).not.toContain(4);
  });
});
