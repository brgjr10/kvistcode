import { ChevronDown, ChevronUp, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { Button } from "../ui/button";

/**
 * Find in conversation (Ctrl+F): the box over the top right of the timeline. Enter goes to the next match,
 * Shift+Enter to the previous one, Esc closes. The timeline owns the query and scrolls to the match.
 */
export function FindBar({
  query,
  onQuery,
  index,
  count,
  onStep,
  onClose,
}: {
  query: string;
  onQuery: (q: string) => void;
  /** The current match (0-based), or -1 when there is none. */
  index: number;
  count: number;
  onStep: (dir: -1 | 1) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  return (
    <div
      role="search"
      className="absolute right-3 top-2 z-20 flex h-9 items-center gap-1 rounded-lg border border-line-strong bg-card pl-2.5 pr-1 shadow-lg"
    >
      <input
        ref={ref}
        data-find-input=""
        value={query}
        onChange={(e) => onQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.nativeEvent.isComposing) {
            e.preventDefault();
            onStep(e.shiftKey ? -1 : 1);
          } else if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            onClose();
          }
        }}
        placeholder="Find in conversation"
        aria-label="Find in conversation"
        className="w-44 bg-transparent text-small text-fg outline-none placeholder:text-steel"
      />
      <span className="min-w-[4.5rem] text-right text-meta tabular-nums text-steel" aria-live="polite">
        {query.trim() ? (count > 0 ? `${index + 1} of ${count}` : "No matches") : ""}
      </span>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Previous match"
        title="Previous match (Shift+Enter)"
        disabled={count === 0}
        onClick={() => onStep(-1)}
      >
        <ChevronUp />
      </Button>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Next match"
        title="Next match (Enter)"
        disabled={count === 0}
        onClick={() => onStep(1)}
      >
        <ChevronDown />
      </Button>
      <Button size="icon-sm" variant="ghost" aria-label="Close find" title="Close (Esc)" onClick={onClose}>
        <X />
      </Button>
    </div>
  );
}

/** Mark every visible occurrence of `query` under `root` (CSS Custom Highlight API; a no-op where missing). */
export function highlightMatches(root: HTMLElement | null, query: string): void {
  const reg = (globalThis as { CSS?: { highlights?: Map<string, unknown> } }).CSS?.highlights;
  const HighlightCtor = (globalThis as { Highlight?: new (...r: Range[]) => unknown }).Highlight;
  if (!reg || !HighlightCtor) return;
  const q = query.trim().toLowerCase();
  if (!root || !q) {
    reg.delete(HIGHLIGHT);
    return;
  }
  const ranges: Range[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n && ranges.length < MAX_MARKS; n = walker.nextNode()) {
    const text = (n.nodeValue ?? "").toLowerCase();
    for (let at = text.indexOf(q); at >= 0 && ranges.length < MAX_MARKS; at = text.indexOf(q, at + q.length)) {
      const r = document.createRange();
      r.setStart(n, at);
      r.setEnd(n, at + q.length);
      ranges.push(r);
    }
  }
  reg.set(HIGHLIGHT, new HighlightCtor(...ranges));
}

export function clearHighlights(): void {
  (globalThis as { CSS?: { highlights?: Map<string, unknown> } }).CSS?.highlights?.delete(HIGHLIGHT);
}

const HIGHLIGHT = "kv-find";
const MAX_MARKS = 400;
