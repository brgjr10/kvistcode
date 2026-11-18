import { describe, expect, it } from "vitest";
import { queryMatcher } from "./agents";
import { seedAgents } from "./mock-data";
import type { Agent } from "./types";

const agent = (over: Partial<Agent>): Agent => ({
  ...seedAgents()[0],
  title: "Fix the build",
  projectPath: "/home/me/code/storefront",
  branch: "kvistcode/fix-build",
  lastMessage: "All 14 checkout specs pass.",
  ...over,
});

describe("queryMatcher (sidebar and Mission Control filter)", () => {
  it("matches everything for an empty or blank query", () => {
    expect(queryMatcher("")(agent({}))).toBe(true);
    expect(queryMatcher("   ")(agent({}))).toBe(true);
  });

  it("matches title, folder, branch and last message, ignoring case and outer spaces", () => {
    expect(queryMatcher("BUILD")(agent({}))).toBe(true);
    expect(queryMatcher("storefront")(agent({}))).toBe(true);
    expect(queryMatcher(" fix-build ")(agent({}))).toBe(true);
    expect(queryMatcher("checkout specs")(agent({}))).toBe(true);
  });

  it("does not match unrelated words or missing fields", () => {
    expect(queryMatcher("billing")(agent({}))).toBe(false);
    expect(queryMatcher("fix-build")(agent({ title: "Other", branch: null }))).toBe(false);
    expect(queryMatcher("specs")(agent({ lastMessage: null }))).toBe(false);
  });
});
