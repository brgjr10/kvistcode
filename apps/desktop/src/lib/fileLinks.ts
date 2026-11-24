// Pure parsing for `file:line` references in markdown timelines and `file://` links.

export interface FileLineRef {
  /** Normalized path (absolute or relative to the agent's cwd). */
  path: string;
  line: number | null;
  column: number | null;
  absolute: boolean;
}

const KNOWN_BARE_NAMES = new Set([
  "makefile",
  "dockerfile",
  "justfile",
  "gemfile",
  "rakefile",
  "procfile",
  "vagrantfile",
  "cmakelists.txt",
]);

const KNOWN_EXT_RE = /\.[a-zA-Z0-9]{1,10}$/;

function isPlausibleFilePath(path: string): boolean {
  if (!path || path.length > 320 || /\s/.test(path)) return false;
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(path)) return false;
  if (path.includes("..")) return false;
  // Exclude host:port lookalikes like localhost or 127.0.0.1 or foo.com without a slash.
  if (!path.includes("/") && !path.includes("\\")) {
    const lower = path.toLowerCase();
    if (lower === "localhost" || /^\d+(?:\.\d+){3}$/.test(lower)) return false;
    if (KNOWN_BARE_NAMES.has(lower)) return true;
    // Require a file extension that isn't a common TLD-only host.
    if (!KNOWN_EXT_RE.test(path)) return false;
    if (/\.(com|org|net|io|dev|app|ai|edu|gov|local)$/i.test(lower)) return false;
    return true;
  }
  const base = path.split(/[\\/]/).pop() ?? "";
  if (!base || base === "." || base === "..") return false;
  return KNOWN_EXT_RE.test(base) || KNOWN_BARE_NAMES.has(base.toLowerCase()) || base.startsWith(".");
}

/**
 * Parses a `file://` URL (with optional `#L42` or `#L42-L50`) into a `FileLineRef`.
 */
export function parseFileUri(href: string): FileLineRef | null {
  const trimmed = href.trim();
  if (!/^file:\/\//i.test(trimmed)) return null;
  const hashIdx = trimmed.indexOf("#");
  const base = hashIdx >= 0 ? trimmed.slice(0, hashIdx) : trimmed;
  const frag = hashIdx >= 0 ? trimmed.slice(hashIdx + 1) : "";

  let rawPath = base.slice("file://".length);
  if (/^localhost\//i.test(rawPath)) {
    rawPath = rawPath.slice("localhost".length);
  }
  if (!rawPath.startsWith("/")) return null;
  let decoded: string;
  try {
    decoded = decodeURIComponent(rawPath);
  } catch {
    return null;
  }
  // Windows file URI: /C:/Users/... -> C:/Users/...
  if (/^\/[A-Za-z]:\//.test(decoded)) {
    decoded = decoded.slice(1);
  }
  let line: number | null = null;
  let column: number | null = null;
  const lm = /^L?(\d+)(?::(\d+))?(?:-L?\d+)?$/i.exec(frag.trim());
  if (lm) {
    const n = Number(lm[1]);
    if (Number.isInteger(n) && n > 0) line = n;
    if (lm[2]) {
      const c = Number(lm[2]);
      if (Number.isInteger(c) && c > 0) column = c;
    }
  }
  return { path: decoded, line, column, absolute: true };
}

/**
 * Parses an inline `path/to/file.ext:42` or `path/to/file.ext:42:7` reference.
 * Returns null for URLs, timestamps, host:port pairs, or strings without a valid file path.
 */
export function parseInlineFileLine(text: string): FileLineRef | null {
  const trimmed = text.trim();
  if (!trimmed || trimmed.length > 360 || /\s/.test(trimmed)) return null;
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmed)) return null;

  // Match trailing :line or :line:col (or :line-endLine).
  const m = /^(.+?):(\d+)(?::(\d+)|-(\d+))?$/.exec(trimmed);
  if (!m) return null;
  const rawPath = m[1];
  const line = Number(m[2]);
  const column = m[3] ? Number(m[3]) : null;
  if (!Number.isInteger(line) || line <= 0 || line > 1_000_000) return null;
  if (column !== null && (!Number.isInteger(column) || column <= 0 || column > 10_000)) return null;
  if (!isPlausibleFilePath(rawPath)) return null;

  const absolute = rawPath.startsWith("/") || /^[A-Za-z]:[\\/]/.test(rawPath);
  const path = absolute ? rawPath : rawPath.replace(/^\.\//, "");
  return { path, line, column, absolute };
}

/** Resolves a `FileLineRef` against an agent's `cwd` to an absolute path. */
export function resolveFileRefPath(ref: FileLineRef, cwd: string): string {
  if (ref.absolute) return ref.path;
  const sep = cwd.includes("\\") && !cwd.includes("/") ? "\\" : "/";
  const base = cwd.replace(/[\\/]+$/, "");
  const rel = ref.path.replace(/^[\\/]+/, "").replace(/[\\/]/g, sep);
  return `${base}${sep}${rel}`;
}

/** Converts an absolute path inside `cwd` to a relative forward-slash path, or null if outside. */
export function toRelativeWorkspacePath(fullPath: string, cwd: string): string | null {
  const normFull = fullPath.replace(/\\/g, "/");
  const normCwd = cwd.replace(/\\/g, "/").replace(/\/+$/, "");
  if (normFull === normCwd) return null;
  if (normFull.startsWith(`${normCwd}/`)) {
    return normFull.slice(normCwd.length + 1);
  }
  return null;
}
