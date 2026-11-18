import { describe, expect, it } from "vitest";
import { lastRunNote } from "./lastRun";

const s = (lastRunAt: number | null, lastAgentId: string | null, lastError: string | null) => ({
  lastRunAt,
  lastAgentId,
  lastError,
});

describe("lastRunNote", () => {
  it("says nothing before the first run", () => {
    expect(lastRunNote(s(null, null, null), false)).toBe("none");
  });

  it("links the agent the run started", () => {
    expect(lastRunNote(s(1, "a1", null), true)).toBe("agent");
  });

  it("says the agent was removed only when there was one", () => {
    expect(lastRunNote(s(1, "a1", null), false)).toBe("removed");
    // A run that is still starting has no agent yet: not "removed".
    expect(lastRunNote(s(1, null, null), false)).toBe("none");
  });

  it("does not link an older run's agent after a failed run", () => {
    expect(lastRunNote(s(2, "a-old", "usage limit"), true)).toBe("failed");
    expect(lastRunNote(s(2, null, "Folder not found"), false)).toBe("failed");
  });
});
