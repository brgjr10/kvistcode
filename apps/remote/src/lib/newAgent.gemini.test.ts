import { describe, expect, it } from "vitest";
import { agentChoices } from "./newAgent";
import type { RemoteAccount } from "./types";

const acc = (id: string, provider: RemoteAccount["provider"]): RemoteAccount => ({ id, name: id, provider, color: "#4285F4", label: null });

describe("agentChoices with Gemini", () => {
  it("lists Gemini as a built-in after Grok and before local models", () => {
    const c = agentChoices([acc("l1", "local"), acc("g1", "gemini"), acc("k1", "grok"), acc("c1", "claude")], []);
    expect(c.map((x) => x.key)).toEqual(["claude", "grok", "gemini", "local"]);
    expect(c.find((x) => x.key === "gemini")).toMatchObject({ name: "Gemini", color: "#4285F4", customAgentId: null });
  });
});
