import {
  Check,
  ChevronRight,
  Code2,
  FileCode2,
  Folder,
  FolderOpen,
  Pencil,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/misc";
import { useHighlight } from "@/hooks/useHighlight";
import { api } from "@/lib/api";
import { resolveFileRefPath } from "@/lib/fileLinks";
import { ancestorDirs, buildFileTree, type FileTreeNode } from "@/lib/fileTree";
import { langFromPath } from "@/lib/highlight";
import type { Agent, WorkspaceFile } from "@/lib/types";
import { cn, errorMessage, modKey } from "@/lib/utils";
import { openPathInEditor, useEditors } from "@/store/editors";
import { agentWs, useWorkspace } from "@/store/workspace";

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Lightweight project file tree, Shiki viewer, and quick inline edit/save inside the WorkspacePanel. */
export function FilesTab({ agent }: { agent: Agent }) {
  const { preferred } = useEditors();
  const selectedFile = useWorkspace((s) => agentWs(s, agent.id).file);
  const selectedLine = useWorkspace((s) => agentWs(s, agent.id).fileLine);
  const setSelectedFile = useWorkspace((s) => s.setFile);

  const [files, setFiles] = useState<string[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  const [doc, setDoc] = useState<WorkspaceFile | null>(null);
  const [docLoading, setDocLoading] = useState(false);
  const [docError, setDocError] = useState<string | null>(null);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState<WorkspaceFile | null>(null);

  const loadList = useCallback(() => {
    setListError(null);
    api
      .listWorkspaceFiles(agent.id)
      .then(setFiles)
      .catch((e: unknown) => setListError(errorMessage(e)));
  }, [agent.id]);

  useEffect(() => {
    setFiles(null);
    loadList();
  }, [loadList]);

  const loadDoc = useCallback(
    (relPath: string) => {
      setDocLoading(true);
      setDocError(null);
      setConflict(null);
      api
        .readWorkspaceFile(agent.id, relPath)
        .then((f) => {
          setDoc(f);
          setDraft(f.content);
          setEditing(false);
        })
        .catch((e: unknown) => {
          setDoc(null);
          setDocError(errorMessage(e));
        })
        .finally(() => setDocLoading(false));
    },
    [agent.id],
  );

  useEffect(() => {
    if (!selectedFile) {
      setDoc(null);
      setDocError(null);
      setEditing(false);
      return;
    }
    // Auto-expand ancestor folders of the opened file.
    const dirs = ancestorDirs(selectedFile);
    if (dirs.length > 0) {
      setExpanded((prev) => {
        const next = new Set(prev);
        for (const d of dirs) next.add(d);
        return next;
      });
    }
    loadDoc(selectedFile);
  }, [selectedFile, loadDoc]);

  const tree = useMemo(() => buildFileTree(files ?? [], query), [files, query]);
  const lang = useMemo(() => langFromPath(doc?.path), [doc?.path]);
  const highlighted = useHighlight(editing || !doc || doc.binary ? null : lang, doc?.content ?? "");
  const plainLines = useMemo(() => (doc && !doc.binary ? doc.content.split("\n") : []), [doc]);

  // Scroll to target line when opened via a file:line link.
  const viewerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!selectedLine || !doc || editing) return;
    const row = viewerRef.current?.querySelector<HTMLElement>(`[data-line="${selectedLine}"]`);
    row?.scrollIntoView({ block: "center" });
  }, [selectedLine, doc, editing]);

  const dirty = editing && doc !== null && draft !== doc.content;

  const save = useCallback(
    async (force = false) => {
      if (!doc || saving) return;
      setSaving(true);
      try {
        const res = await api.saveWorkspaceFile(agent.id, doc.path, draft, force ? null : doc.stamp);
        if (!res.saved) {
          setConflict(res.file);
          toast.warning("File changed on disk", {
            description: "Reload the disk version or choose Save anyway to overwrite.",
          });
          return;
        }
        setDoc(res.file);
        setDraft(res.file.content);
        setConflict(null);
        setEditing(false);
        toast.success(`Saved ${res.file.path}`);
      } catch (e) {
        toast.error("Couldn't save file", { description: errorMessage(e) });
      } finally {
        setSaving(false);
      }
    },
    [agent.id, doc, draft, saving],
  );

  const toggleDir = useCallback((path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const selectFile = useCallback(
    (path: string) => {
      if (dirty && !window.confirm("Discard unsaved edits to this file?")) return;
      setSelectedFile(agent.id, path, null);
    },
    [agent.id, dirty, setSelectedFile],
  );

  const openExternal = (line?: number | null) => {
    if (!doc) return;
    const full = resolveFileRefPath({ path: doc.path, line: null, column: null, absolute: false }, agent.cwd);
    void openPathInEditor(full, { line: line ?? selectedLine ?? null });
  };

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden">
      {/* Left pane: filter + compact file tree */}
      <div className="flex w-56 shrink-0 flex-col border-r border-line bg-panel/35">
        <div className="flex h-8 shrink-0 items-center gap-1 border-b border-line px-1.5">
          <div className="relative flex min-w-0 flex-1 items-center">
            <Search className="pointer-events-none absolute left-1.5 h-3 w-3 text-steel" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter files…"
              aria-label="Filter workspace files"
              className="h-6 w-full rounded-md bg-base/70 pl-6 pr-5 text-caption text-fg placeholder:text-steel outline-none focus:ring-1 focus:ring-accent/50"
            />
            {query && (
              <button
                type="button"
                aria-label="Clear filter"
                onClick={() => setQuery("")}
                className="absolute right-1 text-steel hover:text-fg"
              >
                <X className="h-3 w-3" />
              </button>
            )}
          </div>
          <Button size="icon-xs" variant="ghost" aria-label="Refresh files" onClick={loadList}>
            <RefreshCw />
          </Button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-1 py-1">
          {listError ? (
            <p className="p-2 text-caption text-err">{listError}</p>
          ) : files === null ? (
            <div className="flex items-center gap-1.5 p-2 text-caption text-steel">
              <Spinner className="h-3 w-3" /> Loading files…
            </div>
          ) : tree.length === 0 ? (
            <p className="p-2 text-caption text-steel">{query ? "No matching files." : "Empty folder."}</p>
          ) : (
            <TreeNodes
              nodes={tree}
              depth={0}
              expanded={expanded}
              forceExpand={query.trim().length > 0}
              selected={selectedFile}
              onToggleDir={toggleDir}
              onSelectFile={selectFile}
            />
          )}
        </div>
      </div>

      {/* Right pane: viewer / quick inline editor */}
      <div className="flex min-w-0 flex-1 flex-col bg-code">
        {!selectedFile ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-1.5 px-4 text-center">
            <FileCode2 className="h-5 w-5 text-steel" />
            <p className="text-small text-muted">Pick a file to view or make a quick edit.</p>
          </div>
        ) : (
          <>
            <div className="flex h-8 shrink-0 items-center gap-1.5 border-b border-line bg-panel/60 px-2.5">
              <span className="min-w-0 flex-1 truncate font-mono text-caption text-fg" title={selectedFile}>
                {selectedFile}
                {dirty && <span className="ml-1.5 text-warn">•</span>}
              </span>
              {doc && !doc.binary && !doc.truncated && !editing && (
                <Button size="xs" variant="ghost" onClick={() => setEditing(true)}>
                  <Pencil className="!h-3 !w-3" /> Edit
                </Button>
              )}
              {editing && (
                <>
                  <Button
                    size="xs"
                    variant="secondary"
                    disabled={!dirty || saving}
                    title={`Save (${modKey}+S)`}
                    onClick={() => void save(false)}
                  >
                    {saving ? <Spinner className="h-3 w-3 text-current" /> : <Check className="!h-3 !w-3" />}
                    Save
                  </Button>
                  <Button
                    size="xs"
                    variant="ghost"
                    disabled={saving}
                    onClick={() => {
                      setDraft(doc?.content ?? "");
                      setConflict(null);
                      setEditing(false);
                    }}
                  >
                    Cancel
                  </Button>
                </>
              )}
              {doc && (
                <Button
                  size="xs"
                  variant="ghost"
                  title={preferred ? `Open in ${preferred.name}` : "Open in editor"}
                  onClick={() => openExternal()}
                >
                  <Code2 className="!h-3 !w-3" />
                  <span className="max-sm:hidden">{preferred ? preferred.name : "Editor"}</span>
                </Button>
              )}
              <Button
                size="icon-xs"
                variant="ghost"
                aria-label="Reload file"
                onClick={() => loadDoc(selectedFile)}
              >
                <RefreshCw />
              </Button>
            </div>

            {conflict && (
              <div
                role="alert"
                className="flex flex-wrap items-center gap-2 border-b border-warn/30 bg-warn/10 px-3 py-1.5 text-caption text-warn"
              >
                <span className="flex-1">This file changed on disk since you started editing.</span>
                <Button
                  size="xs"
                  variant="secondary"
                  onClick={() => {
                    setDoc(conflict);
                    setDraft(conflict.content);
                    setConflict(null);
                    setEditing(false);
                  }}
                >
                  Reload disk
                </Button>
                <Button size="xs" variant="warn" disabled={saving} onClick={() => void save(true)}>
                  Save anyway
                </Button>
              </div>
            )}

            {docLoading ? (
              <div className="flex items-center gap-2 p-3 text-small text-steel">
                <Spinner className="h-3.5 w-3.5" /> Reading file…
              </div>
            ) : docError ? (
              <p className="p-3 text-small text-err">{docError}</p>
            ) : !doc ? null : doc.binary ? (
              <div className="flex flex-1 flex-col items-center justify-center gap-2 px-4 text-center">
                <p className="text-small text-muted">Binary file ({formatBytes(doc.sizeBytes)}).</p>
                <Button size="sm" variant="secondary" onClick={() => openExternal()}>
                  <Code2 /> Open in {preferred ? preferred.name : "editor"}
                </Button>
              </div>
            ) : editing ? (
              <textarea
                aria-label={`Edit ${doc.path}`}
                spellCheck={false}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") {
                    e.preventDefault();
                    if (dirty) void save(false);
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    if (!dirty) setEditing(false);
                  }
                }}
                className="min-h-0 flex-1 resize-none bg-code p-3 font-mono text-small leading-[20px] text-fg outline-none"
              />
            ) : (
              <div ref={viewerRef} className="min-h-0 flex-1 overflow-auto">
                {doc.truncated && (
                  <div className="flex items-center justify-between border-b border-line bg-panel/60 px-3 py-1.5 text-caption text-muted">
                    <span>
                      Showing the first 512 KB of {formatBytes(doc.sizeBytes)} (read-only here).
                    </span>
                    <Button size="xs" variant="ghost" onClick={() => openExternal()}>
                      Open in {preferred ? preferred.name : "editor"}
                    </Button>
                  </div>
                )}
                <div className="selectable py-2 font-mono text-small leading-[20px]">
                  {plainLines.map((line, i) => {
                    const lineNo = i + 1;
                    const activeLine = selectedLine === lineNo;
                    const tokens = highlighted?.[i];
                    return (
                      <div
                        key={lineNo}
                        data-line={lineNo}
                        className={cn(
                          "group/line flex px-2",
                          activeLine && "bg-accent/15",
                        )}
                      >
                        <button
                          type="button"
                          title={`Open line ${lineNo} in ${preferred ? preferred.name : "editor"}`}
                          onClick={() => openExternal(lineNo)}
                          className="w-10 shrink-0 select-none pr-3 text-right text-micro tabular-nums text-steel/70 hover:text-accent"
                        >
                          {lineNo}
                        </button>
                        <span className="min-w-0 flex-1 whitespace-pre">
                          {tokens
                            ? tokens.map((tok, ti) => (
                                <span
                                  key={ti}
                                  style={tok.c ? { color: tok.c } : undefined}
                                  className={cn(
                                    tok.s && (tok.s & 1) ? "italic" : undefined,
                                    tok.s && (tok.s & 2) ? "font-semibold" : undefined,
                                    tok.s && (tok.s & 4) ? "underline" : undefined,
                                  )}
                                >
                                  {tok.t}
                                </span>
                              ))
                            : line || " "}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function TreeNodes({
  nodes,
  depth,
  expanded,
  forceExpand,
  selected,
  onToggleDir,
  onSelectFile,
}: {
  nodes: FileTreeNode[];
  depth: number;
  expanded: Set<string>;
  forceExpand: boolean;
  selected: string | null;
  onToggleDir: (path: string) => void;
  onSelectFile: (path: string) => void;
}) {
  return (
    <ul className="space-y-px">
      {nodes.map((node) => {
        const isOpen = node.kind === "dir" && (forceExpand || expanded.has(node.path));
        const isSelected = node.kind === "file" && selected === node.path;
        return (
          <li key={node.path}>
            <button
              type="button"
              onClick={() => (node.kind === "dir" ? onToggleDir(node.path) : onSelectFile(node.path))}
              style={{ paddingLeft: `${depth * 12 + 6}px` }}
              className={cn(
                "flex h-6 w-full items-center gap-1.5 rounded-md pr-2 text-left text-caption transition-colors",
                isSelected ? "bg-accent/15 text-fg font-medium" : "text-muted hover:bg-raised/60 hover:text-fg",
              )}
            >
              {node.kind === "dir" ? (
                <>
                  <ChevronRight
                    className={cn("h-3 w-3 shrink-0 text-steel transition-transform", isOpen && "rotate-90")}
                  />
                  {isOpen ? (
                    <FolderOpen className="h-3.5 w-3.5 shrink-0 text-steel" />
                  ) : (
                    <Folder className="h-3.5 w-3.5 shrink-0 text-steel" />
                  )}
                </>
              ) : (
                <>
                  <span className="w-3 shrink-0" />
                  <FileCode2 className="h-3.5 w-3.5 shrink-0 text-steel" />
                </>
              )}
              <span className="truncate font-mono text-micro">{node.name}</span>
            </button>
            {isOpen && node.children.length > 0 && (
              <TreeNodes
                nodes={node.children}
                depth={depth + 1}
                expanded={expanded}
                forceExpand={forceExpand}
                selected={selected}
                onToggleDir={onToggleDir}
                onSelectFile={onSelectFile}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}
