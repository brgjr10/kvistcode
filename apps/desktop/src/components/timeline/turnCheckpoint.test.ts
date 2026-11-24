import { describe, expect, it } from "vitest";
import type { Checkpoint, TimelineItem } from "@/lib/types";
import { afterTurnCheckpointId, latestTurnPromptId, parseBtwCommand, turnPromptByItemId } from "./turnCheckpoint";

function user(id: string, seq: number, midTurn = false): TimelineItem {
  return {
    id,
    agentId: "a1",
    seq,
    ts: seq * 1000,
    kind: "user_message",
    text: `prompt ${id}`,
    midTurn,
  };
}

function summary(id: string, seq: number): TimelineItem {
  return {
    id,
    agentId: "a1",
    seq,
    ts: seq * 1000,
    kind: "turn_summary",
    durationMs: 1200,
    files: ["a.ts"],
    toolCalls: 1,
    commands: 0,
    costUsd: 0.01,
    tokens: 100,
  };
}

describe("turnCheckpoint helpers", () => {
  it("maps turn items to the user message that started their turn", () => {
    const items: TimelineItem[] = [
      user("u1", 1),
      summary("s1", 2),
      user("u2", 3),
      user("u2-steer", 4, true),
      summary("s2", 5),
    ];
    const map = turnPromptByItemId(items);
    expect(map.get("s1")).toBe("u1");
    expect(map.get("s2")).toBe("u2");
    expect(latestTurnPromptId(items)).toBe("u2");
  });

  it("finds the next turn checkpoint for forking after an earlier turn and null for the latest", () => {
    const ck = (id: string, seq: number, itemId: string): Checkpoint => ({
      id,
      agentId: "a1",
      seq,
      itemId,
      createdAt: seq * 1000,
      kind: "turn",
    });
    const byItem = { u1: ck("ck1", 1, "u1"), u2: ck("ck2", 2, "u2"), u3: ck("ck3", 3, "u3") };
    expect(afterTurnCheckpointId(byItem, "u1")).toBe("ck2");
    expect(afterTurnCheckpointId(byItem, "u2")).toBe("ck3");
    expect(afterTurnCheckpointId(byItem, "u3")).toBeNull();
  });

  it("parses /btw side-chat commands cleanly", () => {
    expect(parseBtwCommand("hello")).toBeNull();
    expect(parseBtwCommand("/compact")).toBeNull();
    expect(parseBtwCommand("/btwextra foo")).toBeNull();
    expect(parseBtwCommand("/btw")).toEqual({ question: "" });
    expect(parseBtwCommand("  /btw   what does foo() return?  ")).toEqual({
      question: "what does foo() return?",
    });
  });
});
