import type { Agent } from "./types";

/** A provider session id is safe on a command line (mirrors the Rust check). */
const SAFE_ID = /^[A-Za-z0-9_-]{1,128}$/;

/**
 * The command that reopens an agent's session in a terminal CLI, or null when Kvistcode doesn't
 * know the provider's own session id (or the provider has no such command).
 */
export function terminalResumeCommand(agent: Pick<Agent, "provider" | "sessionId">): string | null {
  const id = agent.sessionId;
  if (!id || !SAFE_ID.test(id)) return null;
  if (agent.provider === "claude") return `claude --resume ${id}`;
  if (agent.provider === "codex") return `codex resume ${id}`;
  return null;
}
