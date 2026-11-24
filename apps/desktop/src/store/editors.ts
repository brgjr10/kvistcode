import { useEffect } from "react";
import { toast } from "sonner";
import { create } from "zustand";
import { attempt } from "@/lib/actions";
import { api } from "@/lib/api";
import { resolveFileRefPath, toRelativeWorkspacePath, type FileLineRef } from "@/lib/fileLinks";
import type { EditorInfo } from "@/lib/types";
import { errorMessage } from "@/lib/utils";
import { useApp } from "./app";
import { openFileInWorkspace } from "./workspace";

interface EditorsState {
  editors: EditorInfo[];
  loaded: boolean;
  loading: boolean;
  load: (force?: boolean) => Promise<EditorInfo[]>;
}

export const useEditorsStore = create<EditorsState>((set, get) => ({
  editors: [],
  loaded: false,
  loading: false,
  load: async (force = false) => {
    const s = get();
    if ((s.loaded && !force) || s.loading) return s.editors;
    set({ loading: true });
    try {
      const editors = await api.detectEditors();
      set({ editors, loaded: true, loading: false });
      return editors;
    } catch {
      set({ loaded: true, loading: false });
      return get().editors;
    }
  },
}));

/** Loads detected editors once and returns the list plus the currently preferred editor. */
export function useEditors(): { editors: EditorInfo[]; preferred: EditorInfo | null; loaded: boolean } {
  const editors = useEditorsStore((s) => s.editors);
  const loaded = useEditorsStore((s) => s.loaded);
  const preferredId = useApp((s) => s.settings?.preferredEditor ?? null);

  useEffect(() => {
    if (!loaded) void useEditorsStore.getState().load();
  }, [loaded]);

  const preferred =
    (preferredId ? editors.find((e) => e.id === preferredId) : null) ?? editors[0] ?? null;
  return { editors, preferred, loaded };
}

/** Persists the user's preferred editor id in Settings. */
export async function rememberPreferredEditor(editorId: string | null): Promise<void> {
  const settings = useApp.getState().settings;
  if (!settings || (settings.preferredEditor ?? null) === editorId) return;
  await attempt("Couldn't save preferred editor", () =>
    api.updateSettings({ ...settings, preferredEditor: editorId }),
  );
}

/**
 * Opens `path` (folder or file, optionally at `line`/`column`) in the chosen or preferred
 * code editor. When `remember` is true and `editorId` is given, saves it as the preferred editor.
 */
export async function openPathInEditor(
  path: string,
  opts?: { editorId?: string | null; line?: number | null; column?: number | null; remember?: boolean },
): Promise<boolean> {
  try {
    const used = await api.openInEditor(path, {
      editorId: opts?.editorId ?? null,
      line: opts?.line ?? null,
      column: opts?.column ?? null,
    });
    if (opts?.remember && opts.editorId) {
      void rememberPreferredEditor(used.id);
    }
    return true;
  } catch (e) {
    toast.error("Couldn't open in editor", { description: errorMessage(e) });
    return false;
  }
}

/**
 * Opens a `file:line` reference from a timeline or diff. Uses the preferred external editor
 * when one is installed; otherwise opens the file directly in the workspace Files tab.
 */
export async function openFileLineTarget(
  agentId: string | undefined,
  cwd: string | undefined,
  ref: FileLineRef,
  preferInternal = false,
): Promise<void> {
  const fullPath = cwd ? resolveFileRefPath(ref, cwd) : ref.path;
  const rel = cwd ? (ref.absolute ? toRelativeWorkspacePath(ref.path, cwd) : ref.path) : null;

  if (preferInternal && agentId && rel) {
    openFileInWorkspace(agentId, rel, ref.line);
    return;
  }

  const editors = await useEditorsStore.getState().load();
  if (editors.length > 0 && (ref.absolute || cwd)) {
    const ok = await openPathInEditor(fullPath, { line: ref.line, column: ref.column });
    if (ok) return;
  }

  if (agentId && rel) {
    openFileInWorkspace(agentId, rel, ref.line);
    return;
  }

  if (ref.absolute || cwd) {
    await attempt("Couldn't show the file", () => api.openPath(fullPath));
  }
}
