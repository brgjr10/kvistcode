import { startsTurn } from "@/lib/agents";
import type { Checkpoint, TimelineItem } from "@/lib/types";

/**
 * Maps each item index (turn summaries, fold heads, etc.) to the id of the
 * `user_message` that started its turn, so turn-level controls can look up the
 * turn's pre-turn checkpoint in `useCheckpoints`.
 */
export function turnPromptByItemId(items: readonly TimelineItem[]): Map<string, string> {
  const map = new Map<string, string>();
  let currentPromptId: string | null = null;
  for (const it of items) {
    if (it.kind === "user_message" && startsTurn(it)) {
      currentPromptId = it.id;
      map.set(it.id, it.id);
    } else if (currentPromptId) {
      map.set(it.id, currentPromptId);
    }
  }
  return map;
}

/**
 * The id of the last user message that started a turn (used by `TurnEndLine`
 * to offer a Turn diff trigger on the latest finished turn).
 */
export function latestTurnPromptId(items: readonly TimelineItem[]): string | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.kind === "user_message" && startsTurn(it)) return it.id;
  }
  return null;
}

/**
 * Finds the checkpoint representing the state *after* the turn started by
 * `promptItemId`: the next turn checkpoint (`seq > ck.seq`), or `null` when
 * this is the latest turn (meaning "fork from current files").
 */
export function afterTurnCheckpointId(
  byItem: Record<string, Checkpoint> | undefined,
  promptItemId: string | null | undefined,
): string | null {
  if (!byItem || !promptItemId) return null;
  const ck = byItem[promptItemId];
  if (!ck) return null;
  let next: Checkpoint | null = null;
  for (const candidate of Object.values(byItem)) {
    if (candidate.seq > ck.seq && (!next || candidate.seq < next.seq)) {
      next = candidate;
    }
  }
  return next ? next.id : null;
}

/**
 * Parses a `/btw <question>` composer command.
 * Returns `null` if the text is not a `/btw` command; returns `{ question }`
 * (possibly empty) if it is.
 */
export function parseBtwCommand(text: string): { question: string } | null {
  const trimmed = text.trim();
  if (trimmed === "/btw") return { question: "" };
  const m = /^\/btw\s+([\s\S]+)$/.exec(trimmed);
  if (!m) return null;
  return { question: m[1].trim() };
}
