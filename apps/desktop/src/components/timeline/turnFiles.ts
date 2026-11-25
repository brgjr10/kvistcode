import { startsTurn } from "@/lib/agents";
import type { TimelineItem } from "@/lib/types";
import { cheapStats } from "./diffCache";

export interface FileStat {
  additions: number;
  deletions: number;
}

/**
 * The changed file an edit's path names, whether it named it from the project root or in full. An exact
 * match wins; otherwise the closest one (`lib/src/a.ts`, not `src/a.ts`, for `/repo/lib/src/a.ts`).
 */
export function fileFor(files: readonly string[], path: string): string | undefined {
  if (files.includes(path)) return path;
  let best: string | undefined;
  for (const f of files) {
    // The file's path ends the edit's (the edit named it in full): the longer, the closer.
    if (path.endsWith(`/${f}`) && (!best || f.length > best.length)) best = f;
  }
  if (best) return best;
  for (const f of files) {
    // The edit's path ends the file's: the shorter, the closer.
    if (f.endsWith(`/${path}`) && (!best || f.length < best.length)) best = f;
  }
  return best;
}

/**
 * Lines added and removed per changed file of a turn, added up from the edits the agent made in it
 * (sub-agents' edits included). A file no edit named has no entry: its counts are not known.
 */
export function turnFileStats(
  items: readonly TimelineItem[],
  summaryId: string,
  files: readonly string[],
): Map<string, FileStat> {
  const stats = new Map<string, FileStat>();
  const end = items.findLastIndex((it) => it.id === summaryId);
  for (let i = end - 1; i >= 0 && !startsTurn(items[i]); i--) {
    const it = items[i];
    if (it.kind !== "tool_call") continue;
    for (const d of it.diffs) {
      const file = fileFor(files, d.path);
      if (!file) continue;
      const s = cheapStats(d);
      const cur = stats.get(file) ?? { additions: 0, deletions: 0 };
      stats.set(file, { additions: cur.additions + s.additions, deletions: cur.deletions + s.deletions });
    }
  }
  return stats;
}

const isAbsolute = (p: string) => p.startsWith("/") || /^[A-Za-z]:[\\/]/.test(p) || p.startsWith("\\\\");
// Windows paths (a drive letter or \\server) ignore case: C:\Repo and c:\repo are one folder.
const norm = (p: string) => {
  const n = p.replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[A-Za-z]:\//.test(n) || n.startsWith("//") ? n.toLowerCase() : n;
};

/**
 * A turn's changed files split into the agent's own (inside its folder or project; a relative path
 * is inside) and the rest. A sub-agent working in another checkout reports its edits here too, and
 * those must not get Revert or Commit buttons, or count as this agent's change.
 */
export function splitOwnFiles(
  files: readonly string[],
  roots: readonly (string | null | undefined)[],
): { own: string[]; outside: string[] } {
  const dirs = roots.filter((r): r is string => !!r).map((r) => `${norm(r)}/`);
  const own: string[] = [];
  const outside: string[] = [];
  for (const f of files) {
    if (!isAbsolute(f) || dirs.length === 0) own.push(f);
    else (dirs.some((d) => `${norm(f)}/`.startsWith(d)) ? own : outside).push(f);
  }
  return { own, outside };
}
