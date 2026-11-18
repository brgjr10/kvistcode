import type { Schedule } from "@/lib/types";

/**
 * What the "Last run" column shows under the time:
 * - "failed": the last run didn't start an agent (the error line says why). A link would point at an
 *   older run's agent, since a failed run keeps the previous agent id.
 * - "agent": a link to the agent the run started.
 * - "removed": the run started an agent that has since been deleted.
 * - "none": nothing to add (never run, or the run is still starting).
 */
export type LastRunNote = "failed" | "agent" | "removed" | "none";

export function lastRunNote(
  schedule: Pick<Schedule, "lastRunAt" | "lastAgentId" | "lastError">,
  agentExists: boolean,
): LastRunNote {
  if (!schedule.lastRunAt) return "none";
  if (schedule.lastError) return "failed";
  if (!schedule.lastAgentId) return "none";
  return agentExists ? "agent" : "removed";
}
