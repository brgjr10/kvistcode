import { needsAttention } from "@/lib/agents";
import type { Agent, PrStatus } from "@/lib/types";

/** The board's agent columns. Backlog (saved cards) is not an agent state and lives in the database. */
export type AgentColumn = "working" | "review" | "merged";

const QUIET: Agent["status"][] = ["idle", "stopped", "interrupted"];

/**
 * Which column an agent sits in, from its real state (no status of its own):
 *  - merged: its pull request merged (archived or not),
 *  - review: it needs you (the Needs you queue), or it is quiet with an open pull request waiting,
 *  - working: everything else that is not archived.
 * An archived agent that never merged is off the board.
 */
export function columnOf(a: Agent, pr: PrStatus | undefined): AgentColumn | null {
  const info = pr?.state === "ok" ? pr.pr : null;
  if (info?.state === "merged") return "merged";
  if (a.archived) return null;
  if (needsAttention(a) !== null) return "review";
  if (info && (info.state === "open" || info.state === "draft") && QUIET.includes(a.status)) return "review";
  return "working";
}

/** The project's agents per column, newest activity first inside a column. Helper agents are left out. */
export function boardColumns(
  agents: Agent[],
  prs: Record<string, PrStatus>,
  isProject: (a: Agent) => boolean,
): Record<AgentColumn, Agent[]> {
  const out: Record<AgentColumn, Agent[]> = { working: [], review: [], merged: [] };
  for (const a of agents) {
    if (!isProject(a) || a.parentId) continue;
    const col = columnOf(a, prs[a.id]);
    if (col) out[col].push(a);
  }
  for (const list of Object.values(out)) list.sort((x, y) => y.lastActivityAt - x.lastActivityAt);
  return out;
}
