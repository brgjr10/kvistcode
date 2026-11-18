import { describe, expect, it } from "vitest";
import { blockedCopy, shouldAutoRetry } from "./blocked";

describe("blockedCopy", () => {
  it("says the phone is offline instead of blaming the computer", () => {
    const c = blockedCopy("unreachable", false);
    expect(c.offline).toBe(true);
    expect(c.autoRetry).toBe(true);
    expect(c.retryNote).toMatch(/back online/);
  });

  it("an unreachable computer with a network keeps the usual checklist and retries", () => {
    const c = blockedCopy("unreachable", true);
    expect(c.offline).toBe(false);
    expect(c.retryNote).toBe("Retrying automatically…");
  });

  it("only unreachable / remote-off retry by themselves", () => {
    expect(blockedCopy("unavailable", true).autoRetry).toBe(true);
    for (const r of ["rate_limited", "misdirected", "error", null] as const) {
      expect(blockedCopy(r, true).autoRetry).toBe(false);
      expect(blockedCopy(r, true).retryNote).toBeNull();
      // Being offline doesn't change what a rate limit or a server error says.
      expect(blockedCopy(r, false).offline).toBe(false);
    }
  });
});

describe("shouldAutoRetry", () => {
  it("waits for a visible page, a network and no attempt already out", () => {
    expect(shouldAutoRetry(true, true, false)).toBe(true);
    expect(shouldAutoRetry(false, true, false)).toBe(false);
    expect(shouldAutoRetry(true, false, false)).toBe(false);
    expect(shouldAutoRetry(true, true, true)).toBe(false);
  });
});
