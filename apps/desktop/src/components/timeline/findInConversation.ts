import { createContext, useContext } from "react";
import type { TimelineItem } from "@/lib/types";

/**
 * The item holding the current Find match (null while Find is closed or finds nothing). Folded content
 * that holds it opens (a long message, a thought), and marks itself with `data-find-target` so the
 * timeline can bring the exact words into view.
 */
export const FindTargetContext = createContext<FindTarget | null>(null);

export type FindTarget = { id: string; query: string };

/** True when the current Find match is `id` (or one of `ids`, the pieces of a run of messages). */
export function holdsFindTarget(
  target: string | null | undefined,
  id: string,
  parts?: readonly { id: string }[],
): boolean {
  if (!target) return false;
  if (target === id) return true;
  return !!parts && parts.some((p) => p.id === target);
}

export function useFindTarget(id: string, parts?: readonly { id: string }[]): boolean {
  return holdsFindTarget(useContext(FindTargetContext)?.id, id, parts);
}

type KeyLike = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

/**
 * The conversation's own keys: Find (⌘F on a Mac, Ctrl+F elsewhere; Ctrl+F is "forward a character" in
 * a Mac text field) and the previous / next message you sent (Alt+Shift+Up / Down; in a Mac text field
 * ⌥⇧↑ selects to the paragraph start, so it stays the field's there).
 */
export function conversationKey(
  e: KeyLike,
  mac: boolean,
  editable: boolean,
): "find" | "prev-message" | "next-message" | null {
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") return "find";
  if (e.altKey && e.shiftKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
    if (mac && editable) return null;
    return e.key === "ArrowUp" ? "prev-message" : "next-message";
  }
  return null;
}

/** The words of an item that Find in conversation searches: what was said, and what each step was. */
export function searchableText(it: TimelineItem): string {
  switch (it.kind) {
    case "user_message":
    case "agent_message":
    case "thought":
      return it.text;
    case "tool_call":
      return it.command ? `${it.title}\n${it.command}` : it.title;
    case "notice":
      return it.text;
    case "plan":
      return it.entries.map((e) => e.content).join("\n");
    case "permission":
      return [it.title, it.command ?? "", it.plan ?? ""].join("\n");
    case "question":
      return it.message;
    default:
      return "";
  }
}

/** Ids of the items that contain `query` (ignoring case), in timeline order. Blank queries match nothing. */
export function findMatches(items: readonly TimelineItem[], query: string): string[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const out: string[] = [];
  for (const it of items) if (searchableText(it).toLowerCase().includes(q)) out.push(it.id);
  return out;
}

/**
 * The row to jump to for "previous / next message you sent": the nearest of `userRows` (row indices of
 * your messages, ascending) before or after `current` (the row at the top of the view), or null at the end.
 */
export function stepUserRow(userRows: readonly number[], current: number, dir: -1 | 1): number | null {
  if (dir < 0) {
    for (let i = userRows.length - 1; i >= 0; i--) if (userRows[i] < current) return userRows[i];
    return null;
  }
  for (const r of userRows) if (r > current) return r;
  return null;
}

/** "2 of 5", or "2 of 5+" while earlier messages aren't loaded (there may be more). */
export function findCountLabel(query: string, index: number, count: number, partial = false): string {
  if (!query.trim()) return "";
  if (count === 0) return partial ? "None loaded" : "No matches";
  // (Between a new query and its first jump the old position may not fit the new count.)
  const at = index < 0 || index >= count ? "–" : String(index + 1);
  return `${at} of ${count}${partial ? "+" : ""}`;
}
