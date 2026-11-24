import { describe, expect, it } from "vitest";
import { seedAgents } from "@/lib/mock-data";
import type { Agent, PrInfo, PrStatus } from "@/lib/types";
import { boardColumns, columnOf } from "./columns";

const agent = (over: Partial<Agent>): Agent => ({
  ...seedAgents()[0],
  id: "a",
  archived: false,
  unread: false,
  pendingPermissions: 0,
  rateLimited: false,
  budgetHold: undefined,
  acknowledgedAt: null,
  parentId: null,
  status: "working",
  ...over,
});
const pr = (state: PrInfo["state"]): PrStatus => ({
  state: "ok",
  reason: null,
  fetchedAt: 0,
  pr: {
    number: 1,
    url: "",
    title: "",
    state,
    review: null,
    mergeable: null,
    checks: [],
    summary: { passing: 0, failing: 0, pending: 0, overall: "none" },
  },
});

describe("columnOf", () => {
  it("puts running and quiet agents under Working", () => {
    expect(columnOf(agent({ status: "working" }), undefined)).toBe("working");
    expect(columnOf(agent({ status: "idle" }), undefined)).toBe("working");
  });
  it("puts agents that need you under Needs review", () => {
    expect(columnOf(agent({ status: "needs_permission", pendingPermissions: 1 }), undefined)).toBe("review");
    expect(columnOf(agent({ status: "idle", unread: true }), undefined)).toBe("review");
    expect(columnOf(agent({ status: "error" }), undefined)).toBe("review");
  });
  it("keeps a quiet agent with an open pull request in review, but not a busy one", () => {
    expect(columnOf(agent({ status: "idle" }), pr("open"))).toBe("review");
    expect(columnOf(agent({ status: "working" }), pr("draft"))).toBe("working");
  });
  it("puts merged pull requests under Merged, archived or not", () => {
    expect(columnOf(agent({}), pr("merged"))).toBe("merged");
    expect(columnOf(agent({ archived: true }), pr("merged"))).toBe("merged");
  });
  it("leaves an archived agent that never merged off the board", () => {
    expect(columnOf(agent({ archived: true }), pr("open"))).toBeNull();
    expect(columnOf(agent({ archived: true }), undefined)).toBeNull();
  });
});

describe("boardColumns", () => {
  it("keeps to the project, skips helpers and sorts by recent activity", () => {
    const list = [
      agent({ id: "1", lastActivityAt: 1 }),
      agent({ id: "2", lastActivityAt: 5 }),
      agent({ id: "3", parentId: "1" }),
      agent({ id: "4", projectPath: "/elsewhere" }),
    ];
    const cols = boardColumns(list, {}, (a) => a.projectPath !== "/elsewhere");
    expect(cols.working.map((a) => a.id)).toEqual(["2", "1"]);
    expect(cols.review).toEqual([]);
  });
});
