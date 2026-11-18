import type { TimelineItem } from "@/lib/types";

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
