import { describe, expect, it } from "vitest";
import { authHelp, firedToday, starterPrompt } from "./triggers";

describe("firedToday", () => {
  const day = new Date(2026, 10, 24, 12);
  it("counts only runs from today", () => {
    expect(firedToday({ firedDay: "2026-11-24", firedToday: 3 }, day)).toBe(3);
    expect(firedToday({ firedDay: "2026-11-23", firedToday: 3 }, day)).toBe(0);
    expect(firedToday({ firedDay: "", firedToday: 0 }, day)).toBe(0);
  });
});

describe("starterPrompt", () => {
  it("always places the event in the prompt", () => {
    for (const s of ["github_ci", "github_mention", "webhook"] as const) {
      expect(starterPrompt(s)).toContain("{{event}}");
    }
  });
});

describe("authHelp", () => {
  it("names the header each mode uses", () => {
    expect(authHelp("hmac")).toContain("X-Hub-Signature-256");
    expect(authHelp("bearer")).toContain("Authorization: Bearer");
  });
});
