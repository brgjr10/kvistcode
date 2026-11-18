// The compact Mission Control row shows an idle agent's last message as one plain line, like the tile does.
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { seedAgents } from "@/lib/mock-data";
import type { Agent } from "@/lib/types";

vi.mock("@/lib/api", () => ({ api: {} }));
vi.mock("@/hooks/useNow", () => ({ useNow: () => 1_700_000_000_000 }));

const { TooltipProvider } = await import("@/components/ui/tooltip");
const { AgentRowCompact } = await import("./AgentRowCompact");

const summary = { action: null, message: null, messageIsTail: false, tail: null, subagents: 0 };
const agent = (over: Partial<Agent>): Agent => ({
  ...seedAgents()[0],
  id: "a",
  title: "Fix the build",
  status: "idle",
  rateLimited: false,
  running: false,
  ...over,
});

const row = (a: Agent) =>
  renderToStaticMarkup(
    createElement(
      TooltipProvider,
      null,
      createElement(AgentRowCompact, { agent: a, account: undefined, summary, pending: null, onOpen: () => {} }),
    ),
  );

describe("AgentRowCompact last message", () => {
  it("drops markdown marks from the last message", () => {
    const html = row(agent({ lastMessage: "## Done\n\n**All 14 specs pass.** Run `npm test` to check." }));
    expect(html).toContain("Done All 14 specs pass. Run npm test to check.");
    expect(html).not.toContain("**");
    expect(html).not.toContain("##");
  });
});
