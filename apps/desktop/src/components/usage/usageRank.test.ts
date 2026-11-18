import { describe, expect, it } from "vitest";
import { foldRest, topAgents, usedBy } from "./usageRank";

const a = (id: string, costUsd: number, tokensUsed: number) => ({ id, costUsd, tokensUsed });

describe("topAgents", () => {
  const agents = [a("x", 1, 50), a("y", 3, 10), a("z", 0, 0), a("w", 2, 900)];

  it("ranks by cost", () => {
    expect(topAgents(agents, "cost").map((x) => x.id)).toEqual(["y", "w", "x"]);
  });

  it("ranks by tokens when that is the metric", () => {
    expect(topAgents(agents, "tokens").map((x) => x.id)).toEqual(["w", "x", "y"]);
  });

  it("leaves out agents that used nothing and caps the list", () => {
    expect(topAgents(agents, "cost", 2).map((x) => x.id)).toEqual(["y", "w"]);
    expect(topAgents([a("z", 0, 0)], "cost")).toEqual([]);
  });

  it("does not reorder the input", () => {
    const copy = [...agents];
    topAgents(agents, "tokens");
    expect(agents).toEqual(copy);
  });

  it("reads the right field", () => {
    expect(usedBy(a("q", 1.5, 7), "cost")).toBe(1.5);
    expect(usedBy(a("q", 1.5, 7), "tokens")).toBe(7);
  });
});

describe("foldRest", () => {
  const b = (key: string, cost: number, tokens: number, agents = 1) => ({ key, cost, tokens, agents });

  it("keeps a short list as it is", () => {
    const list = [b("a", 1, 1), b("b", 2, 2)];
    expect(foldRest(list, 2)).toEqual({ shown: list, rest: null });
  });

  it("sums what is past the cut into one bucket", () => {
    const list = [b("a", 5, 50), b("b", 3, 30), b("c", 1, 10, 2), b("d", 0.5, 5)];
    const { shown, rest } = foldRest(list, 2);
    expect(shown.map((x) => x.key)).toEqual(["a", "b"]);
    expect(rest).toEqual({ key: "__rest__", count: 2, cost: 1.5, tokens: 15, agents: 3 });
  });
});
