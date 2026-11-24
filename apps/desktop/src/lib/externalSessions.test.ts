import { describe, expect, it } from "vitest";
import { terminalResumeCommand } from "./externalSessions";

describe("terminalResumeCommand", () => {
  it("builds the CLI command for Claude and Codex", () => {
    expect(terminalResumeCommand({ provider: "claude", sessionId: "abc-123" })).toBe("claude --resume abc-123");
    expect(terminalResumeCommand({ provider: "codex", sessionId: "abc-123" })).toBe("codex resume abc-123");
  });
  it("offers nothing without a known, safe session id", () => {
    expect(terminalResumeCommand({ provider: "claude", sessionId: null })).toBeNull();
    expect(terminalResumeCommand({ provider: "claude", sessionId: "a; rm -rf ~" })).toBeNull();
  });
  it("offers nothing for other providers", () => {
    expect(terminalResumeCommand({ provider: "grok", sessionId: "abc" })).toBeNull();
  });
});
