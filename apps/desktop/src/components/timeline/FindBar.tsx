import { ChevronDown, ChevronUp, X } from "lucide-react";
import { useEffect, useRef } from "react";
import { Button } from "../ui/button";
import { findCountLabel } from "./findInConversation";

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
  partial = false,
}: {
  query: string;
  onQuery: (q: string) => void;
  /** The current match (0-based), or -1 when there is none. */
  index: number;
  count: number;
  onStep: (dir: -1 | 1) => void;
  onClose: () => void;
  /** Earlier messages are not loaded yet, so there may be more matches than counted. */
  partial?: boolean;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  return (
    <div
      role="search"
      className="absolute right-3 top-2 z-20 flex h-9 max-w-[calc(100%-1.5rem)] items-center gap-1 rounded-lg border border-line-strong bg-card pl-2.5 pr-1 shadow-lg"
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
        className="w-44 min-w-0 bg-transparent text-small text-fg outline-none placeholder:text-steel"
      />
      <span
        className="min-w-[4.5rem] shrink-0 text-right text-meta tabular-nums text-steel"
        aria-live="polite"
        title={partial && query.trim() ? "Only loaded messages are searched; scroll up to load earlier ones" : undefined}
      >
        {findCountLabel(query, index, count, partial)}
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

type Registry = Map<string, unknown>;
type HighlightCtor = new (...r: Range[]) => unknown;

function highlightApi(): { reg: Registry; Ctor: HighlightCtor } | null {
  const reg = (globalThis as { CSS?: { highlights?: Registry } }).CSS?.highlights;
  const Ctor = (globalThis as { Highlight?: HighlightCtor }).Highlight;
  return reg && Ctor ? { reg, Ctor } : null;
}

/** Each open Find's marks, by pane (side by side, closing one pane's Find must not wipe the other's). */
const owned = new Map<string, { all: Range[]; current: Range[] }>();

function publish(): void {
  const api = highlightApi();
  if (!api) return;
  const all = [...owned.values()].flatMap((o) => o.all);
  const current = [...owned.values()].flatMap((o) => o.current);
  if (all.length) api.reg.set(HIGHLIGHT, new api.Ctor(...all));
  else api.reg.delete(HIGHLIGHT);
  if (current.length) api.reg.set(HIGHLIGHT_CURRENT, new api.Ctor(...current));
  else api.reg.delete(HIGHLIGHT_CURRENT);
}

/** Ranges of `q` (trimmed and lower-cased) in the text under `root`, at most `max`. */
function rangesOf(root: Node, q: string, max: number): Range[] {
  const ranges: Range[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n && ranges.length < max; n = walker.nextNode()) {
    const text = (n.nodeValue ?? "").toLowerCase();
    for (let at = text.indexOf(q); at >= 0 && ranges.length < max; at = text.indexOf(q, at + q.length)) {
      const r = document.createRange();
      r.setStart(n, at);
      r.setEnd(n, at + q.length);
      ranges.push(r);
    }
  }
  return ranges;
}

/** The first occurrence of `query` under `el` (ignoring case), or null. */
export function firstMatchIn(el: Node | null, query: string): Range | null {
  const q = query.trim().toLowerCase();
  if (!el || !q) return null;
  return rangesOf(el, q, 1)[0] ?? null;
}

/** The innermost element marked as holding the current match (a row, and the message inside it). */
export function findTargetEl(root: ParentNode | null): HTMLElement | null {
  const all = root?.querySelectorAll<HTMLElement>("[data-find-target]");
  return all && all.length ? all[all.length - 1] : null;
}

/**
 * Mark every visible occurrence of `query` under `root` (CSS Custom Highlight API; a no-op where missing).
 * The first one inside the current match's item (`[data-find-target]`) is marked apart, as the current one.
 * `owner` keeps each pane's marks apart.
 */
export function highlightMatches(root: HTMLElement | null, query: string, owner = ""): void {
  if (!highlightApi()) return;
  const q = query.trim().toLowerCase();
  if (!root || !q) {
    clearHighlights(owner);
    return;
  }
  const ranges = rangesOf(root, q, MAX_MARKS);
  const current = firstMatchIn(findTargetEl(root), q);
  const isCurrent = (r: Range) =>
    !!current && r.startContainer === current.startContainer && r.startOffset === current.startOffset;
  owned.set(owner, { all: ranges.filter((r) => !isCurrent(r)), current: current ? [current] : [] });
  publish();
}

export function clearHighlights(owner = ""): void {
  if (owned.delete(owner)) publish();
}

const HIGHLIGHT = "kv-find";
const HIGHLIGHT_CURRENT = "kv-find-current";
const MAX_MARKS = 400;
