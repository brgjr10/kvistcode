// Ranking helpers for the Usage page: the agents that used the most, and a short list with the rest folded in.

export type UsageMetric = "cost" | "tokens";

interface Used {
  costUsd: number;
  tokensUsed: number;
}

export function usedBy(a: Used, metric: UsageMetric): number {
  return metric === "cost" ? a.costUsd : a.tokensUsed;
}

/** The `n` agents that used the most by `metric`, biggest first. Agents that used nothing are left out. */
export function topAgents<T extends Used>(agents: T[], metric: UsageMetric, n = 8): T[] {
  return agents
    .filter((a) => usedBy(a, metric) > 0)
    .sort((a, b) => usedBy(b, metric) - usedBy(a, metric))
    .slice(0, n);
}

export interface RankBucket {
  key: string;
  cost: number;
  tokens: number;
  agents: number;
}

/**
 * The first `n` buckets as given, plus the rest summed into one bucket (null when nothing is left over),
 * so a short list still adds up to the whole.
 */
export function foldRest<T extends RankBucket>(sorted: T[], n: number): { shown: T[]; rest: (RankBucket & { count: number }) | null } {
  if (sorted.length <= n) return { shown: sorted, rest: null };
  const tail = sorted.slice(n);
  return {
    shown: sorted.slice(0, n),
    rest: {
      key: "__rest__",
      count: tail.length,
      cost: tail.reduce((s, b) => s + b.cost, 0),
      tokens: tail.reduce((s, b) => s + b.tokens, 0),
      agents: tail.reduce((s, b) => s + b.agents, 0),
    },
  };
}
