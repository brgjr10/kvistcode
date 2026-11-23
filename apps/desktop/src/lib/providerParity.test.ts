import { describe, expect, it } from "vitest";
import { subagentRuns } from "./agents";
import { inheritedOf, subagentChips } from "@/components/timeline/thinking";
import type { ConfigOption, TimelineItem } from "./types";

const BASE = 1_800_000_000_000;

function call(
  id: string,
  title: string,
  over: Partial<Extract<TimelineItem, { kind: "tool_call" }>> = {},
): Extract<TimelineItem, { kind: "tool_call" }> {
  return {
    id,
    agentId: "a1",
    seq: 1,
    ts: BASE,
    kind: "tool_call",
    toolCallId: id,
    title,
    toolKind: "other",
    status: "completed",
    locations: [],
    diffs: [],
    output: "",
    command: null,
    ...over,
  };
}

describe("non-Claude subagent parity", () => {
  it("tracks Antigravity background invoke_subagent runs and stops them on manage_subagents kill", () => {
    const prompt: TimelineItem = {
      id: "u1",
      agentId: "a1",
      seq: 0,
      ts: BASE - 1_000,
      kind: "user_message",
      text: "audit providers",
      attachments: [],
    };
    const launch = call("sub_1", "invoke_subagent", {
      ts: BASE,
      toolKind: "think",
      status: "completed",
      output: '{"conversationId":"conv-42"}',
      subagent: { kind: "research", description: "Audit providers", background: true },
    });
    const running = subagentRuns([prompt, launch], false, { now: BASE + 10_000, live: true });
    expect(running).toHaveLength(1);
    expect(running[0].state).toBe("running");

    const kill = call("kill_1", "manage_subagents", {
      ts: BASE + 12_000,
      status: "completed",
      output: 'Killed subagent conversationId: "conv-42"',
    });
    const afterKill = subagentRuns([prompt, launch, kill], false, { now: BASE + 15_000, live: true });
    expect(afterKill).toHaveLength(1);
    expect(afterKill[0].state).toBe("done");
  });

  it("derives inherited model and effort chips for Gemini, Codex, Grok and Local agents", () => {
    const opts: ConfigOption[] = [
      { id: "model", name: "Model", category: null, currentValue: "gemini-3-pro", options: [{ value: "gemini-3-pro", name: "Gemini 3 Pro", description: null }] },
      { id: "thinking_level", name: "Thinking", category: null, currentValue: "high", options: [{ value: "high", name: "High", description: null }] },
    ];
    const inherited = inheritedOf(opts);
    expect(subagentChips({ kind: "research" }, inherited)).toEqual([
      { key: "model", text: "inherits Gemini 3 Pro", inherited: true },
      { key: "effort", text: "inherits High", inherited: true },
    ]);
    // Local agents have no model configOption; fallbackModel supplies their model.
    expect(subagentChips({ kind: "worker" }, inheritedOf([], undefined, "qwen3-coder:32b"))).toEqual([
      { key: "model", text: "inherits qwen3-coder:32b", inherited: true },
    ]);
  });
});
