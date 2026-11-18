import { describe, expect, it } from "vitest";
import type { Schedule } from "@/lib/types";
import { duplicateSchedule } from "./duplicate";

const base: Schedule = {
  id: "s1",
  name: "Nightly audit",
  enabled: true,
  customAgentId: null,
  provider: "claude",
  accountId: "acc",
  projectPath: "/p",
  prompt: "Run npm audit",
  worktree: true,
  time: "02:00",
  days: [1, 2],
  lastRunAt: 5,
  lastAgentId: "a1",
  lastError: "boom",
  permissionMode: "ask",
  updatedAt: 1,
};

describe("duplicateSchedule", () => {
  it("makes an unsaved, paused copy without the run record", () => {
    const c = duplicateSchedule(base, 99);
    expect(c).toMatchObject({
      id: "",
      name: "Nightly audit (copy)",
      enabled: false,
      lastRunAt: null,
      lastAgentId: null,
      lastError: null,
      updatedAt: 99,
    });
  });

  it("keeps what the schedule does", () => {
    const c = duplicateSchedule(base, 99);
    expect(c).toMatchObject({ provider: "claude", accountId: "acc", prompt: "Run npm audit", time: "02:00" });
    expect(c.days).toEqual([1, 2]);
    expect(c.days).not.toBe(base.days);
  });

  it("leaves an unnamed schedule unnamed", () => {
    expect(duplicateSchedule({ ...base, name: "" }).name).toBe("");
  });
});
