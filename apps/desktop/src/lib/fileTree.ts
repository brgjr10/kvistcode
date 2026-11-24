// Pure file-tree builder for the lightweight workspace Files tab.

export interface FileTreeNode {
  name: string;
  /** Full forward-slash relative path from workspace root. */
  path: string;
  kind: "dir" | "file";
  children: FileTreeNode[];
}

/**
 * Builds a sorted directory-first tree from a flat list of relative file paths.
 * When `query` is non-empty, only files whose path contains all whitespace-separated
 * terms (case-insensitive) are included.
 */
export function buildFileTree(files: readonly string[], query = ""): FileTreeNode[] {
  const terms = query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
  const filtered =
    terms.length === 0
      ? files
      : files.filter((p) => {
          const lower = p.toLowerCase();
          return terms.every((t) => lower.includes(t));
        });

  const root: FileTreeNode = { name: "", path: "", kind: "dir", children: [] };
  const dirMap = new Map<string, FileTreeNode>();
  dirMap.set("", root);

  for (const raw of filtered) {
    const clean = raw.replace(/\\/g, "/").replace(/^\/+/, "");
    if (!clean) continue;
    const parts = clean.split("/");
    let parent = root;
    let prefix = "";
    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      if (!part) continue;
      prefix = prefix ? `${prefix}/${part}` : part;
      const isFile = i === parts.length - 1;
      if (isFile) {
        parent.children.push({ name: part, path: prefix, kind: "file", children: [] });
      } else {
        let dir = dirMap.get(prefix);
        if (!dir) {
          dir = { name: part, path: prefix, kind: "dir", children: [] };
          dirMap.set(prefix, dir);
          parent.children.push(dir);
        }
        parent = dir;
      }
    }
  }

  sortTree(root.children);
  return root.children;
}

function sortTree(nodes: FileTreeNode[]) {
  nodes.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
  });
  for (const n of nodes) {
    if (n.kind === "dir") sortTree(n.children);
  }
}

/** Returns every ancestor directory path of a relative file path (e.g. "a/b/c.ts" -> ["a", "a/b"]). */
export function ancestorDirs(relPath: string): string[] {
  const parts = relPath.replace(/\\/g, "/").split("/");
  const out: string[] = [];
  let cur = "";
  for (let i = 0; i < parts.length - 1; i++) {
    cur = cur ? `${cur}/${parts[i]}` : parts[i];
    out.push(cur);
  }
  return out;
}
