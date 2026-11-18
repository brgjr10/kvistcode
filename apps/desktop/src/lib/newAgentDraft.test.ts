import { beforeEach, describe, expect, it } from "vitest";
import { initialPrompt, keepUnsentPrompt, launchBlocker } from "./newAgentDraft";

const ready = {
  project: "/work/app",
  compare: false,
  chosen: 1,
  launched: 0,
  prompt: "",
  provider: "claude" as const,
  model: "",
  notGit: false,
};

describe("launchBlocker", () => {
  it("lets a single agent start without a prompt", () => {
    expect(launchBlocker(ready)).toBeNull();
  });

  it("asks for a folder first", () => {
    expect(launchBlocker({ ...ready, project: "   " })).toBe("Choose a project folder");
  });

  it("asks for an account when none is picked", () => {
    expect(launchBlocker({ ...ready, chosen: 0 })).toBe("Choose an account");
    expect(launchBlocker({ ...ready, chosen: 0, provider: "local" })).toBe("Add a local model server");
  });

  it("explains that several agents need git", () => {
    expect(launchBlocker({ ...ready, notGit: true })).toBe("Several agents need a git repository");
  });

  it("asks a local agent for its model", () => {
    expect(launchBlocker({ ...ready, provider: "local" })).toBe("Choose a model");
    expect(launchBlocker({ ...ready, provider: "local", model: "qwen3" })).toBeNull();
  });

  describe("compare mode", () => {
    const cmp = { ...ready, compare: true, chosen: 2, prompt: "Fix the bug" };
    it("needs two agents and a prompt", () => {
      expect(launchBlocker(cmp)).toBeNull();
      expect(launchBlocker({ ...cmp, chosen: 1 })).toBe("Pick at least two agents to compare");
      expect(launchBlocker({ ...cmp, prompt: " " })).toBe("Write the prompt to compare");
    });
    it("retries a single failed agent after a partial launch", () => {
      expect(launchBlocker({ ...cmp, chosen: 1, launched: 2 })).toBeNull();
    });
    it("still needs something to start after a partial launch", () => {
      expect(launchBlocker({ ...cmp, chosen: 0, launched: 2 })).toBe("Pick the agents to start");
    });
    it("doesn't ask a compare launch for a model", () => {
      expect(launchBlocker({ ...cmp, provider: "local" })).toBeNull();
    });
  });
});

describe("unsent prompt", () => {
  beforeEach(() => keepUnsentPrompt(""));

  it("starts empty", () => {
    expect(initialPrompt(undefined)).toBe("");
  });

  it("comes back when the dialog opens again", () => {
    keepUnsentPrompt("Refactor the parser");
    expect(initialPrompt(undefined)).toBe("Refactor the parser");
    expect(initialPrompt("")).toBe("Refactor the parser");
  });

  it("gives way to a prompt the dialog is opened with", () => {
    keepUnsentPrompt("old idea");
    expect(initialPrompt("From the palette")).toBe("From the palette");
  });

  it("forgets whitespace and a launched prompt", () => {
    keepUnsentPrompt("  \n ");
    expect(initialPrompt(undefined)).toBe("");
    keepUnsentPrompt("sent");
    keepUnsentPrompt("");
    expect(initialPrompt(undefined)).toBe("");
  });
});
