import type { TimelineItem } from "@/lib/types";

/**
 * The newest error notice of the current turn (nothing before the last message sent), as the limit
 * banner above the message box reads it. Any notice would do before, so an unrelated info line could
 * stand in for the provider's own "usage limit reached" wording.
 */
export function turnErrorNotice(items: readonly TimelineItem[] | undefined): string | null {
  if (!items) return null;
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i];
    if (it.kind === "notice" && it.level === "error") return it.text;
    if (it.kind === "user_message") return null;
  }
  return null;
}
