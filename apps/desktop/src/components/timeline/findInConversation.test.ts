import { describe, expect, it } from "vitest";
import type { TimelineItem } from "@/lib/types";
import { findMatches, stepUserRow } from "./findInConversation";

const user = (id: string, text: string) => ({ kind: "user_message", id, text, ts: 0 }) as unknown as TimelineItem;
const reply = (id: string, text: string) => ({ kind: "agent_message", id, text, ts: 0 }) as unknown as TimelineItem;
const tool = (id: string, title: string, command: string | null = null) =>
  ({ kind: "tool_call", id, title, command, ts: 0 }) as unknown as TimelineItem;

describe("findMatches", () => {
  const items = [
    user("u1", "Fix the Webhook handler"),
    tool("t1", "Read src/webhooks/stripe.ts"),
    tool("t2", "Run tests", "npm test -- webhook"),
    reply("a1", "The handler is fixed."),
  ];

  it("finds what was said and what each step was, ignoring case, in order", () => {
    expect(findMatches(items, "webhook")).toEqual(["u1", "t1", "t2"]);
    expect(findMatches(items, "HANDLER")).toEqual(["u1", "a1"]);
  });

  it("matches nothing for a blank query", () => {
    expect(findMatches(items, "   ")).toEqual([]);
  });
});

describe("stepUserRow", () => {
  const rows = [2, 9, 15];
  it("goes to the message before or after the row in view", () => {
    expect(stepUserRow(rows, 9, -1)).toBe(2);
    expect(stepUserRow(rows, 9, 1)).toBe(15);
    expect(stepUserRow(rows, 10, -1)).toBe(9);
    expect(stepUserRow(rows, 0, 1)).toBe(2);
  });
  it("stops at either end", () => {
    expect(stepUserRow(rows, 2, -1)).toBeNull();
    expect(stepUserRow(rows, 15, 1)).toBeNull();
  });
});
