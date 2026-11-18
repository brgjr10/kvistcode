import { useCallback, useRef, useState, type ClipboardEvent, type DragEvent, type SetStateAction } from "react";
import { toast } from "sonner";
import {
  ACCEPTED_IMAGE_LABEL,
  ACCEPTED_IMAGE_TYPES,
  filesToAttachments,
  hasFiles,
  hasText,
  isFileListPaste,
  imageFilesFrom,
  MAX_ATTACHMENTS,
  mergeAttachments,
  skippedNotes,
} from "@/lib/attachments";
import { getDraftAttachments, setDraftAttachments } from "@/components/agent/drafts";
import { api } from "@/lib/api";
import type { Attachment } from "@/lib/types";

/**
 * Pending image attachments for a prompt: paste, drag & drop and a file picker,
 * with the 8 images / 5 MB / images-only limits enforced and reported via toast.
 */
export function useAttachments(initial: Attachment[] = [], agentId?: string) {
  const [attachments, setRaw] = useState<Attachment[]>(initial);
  const [dragging, setDragging] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const depth = useRef(0);
  // The current list, updated synchronously on every change (not from render), so two quick adds
  // can't both see the same free room and silently overflow, and a repeat is noticed at once.
  const live = useRef(initial);
  // Which agent the composer shows now: a read that finishes after a switch goes to the agent it was for.
  const currentAgent = useRef(agentId);
  currentAgent.current = agentId;

  const setAttachments = useCallback(
    (next: SetStateAction<Attachment[]>) => {
      const v = typeof next === "function" ? next(live.current) : next;
      live.current = v;
      // An agent's composer remembers its pictures while you look at another agent. Stored here
      // too, so a failed send hands them back even after the composer is gone.
      if (agentId) setDraftAttachments(agentId, v);
      setRaw(v);
    },
    [agentId],
  );

  /**
   * Adds pictures already read (a paste, a screenshot, a message taken back): repeats are skipped and
   * the per-message limit kept. Returns why some weren't added.
   */
  const merge = useCallback(
    (read: Attachment[]): string[] => {
      const { next, duplicates, dropped } = mergeAttachments(live.current, read);
      if (next.length !== live.current.length) setAttachments(next);
      return skippedNotes(duplicates, dropped);
    },
    [setAttachments],
  );

  const add = useCallback(
    async (files: File[]) => {
      if (files.length === 0) return;
      const { attachments: read, problems } = await filesToAttachments(files, live.current.length);
      if (agentId && currentAgent.current !== agentId) {
        // The composer moved to another agent while reading: keep the pictures with the one they were added to.
        setDraftAttachments(agentId, mergeAttachments(getDraftAttachments(agentId), read).next);
        return;
      }
      // Merged after the async read: another add may have used the room meanwhile.
      reportProblems(problems, merge(read), "files");
    },
    [merge, agentId],
  );

  const remove = useCallback((i: number) => setAttachments((cur) => cur.filter((_, j) => j !== i)), [setAttachments]);
  const clear = useCallback(() => setAttachments([]), [setAttachments]);
  const openPicker = useCallback(() => inputRef.current?.click(), []);

  /** Native fallback: WebKitGTK/Wayland often hands the paste event no image file. */
  const addFromClipboard = useCallback(async () => {
    try {
      const { attachments: read, problems } = await api.readClipboardImages(agentId);
      reportProblems(problems, merge(read), "images");
    } catch {
      /* no native clipboard support: the paste simply does nothing */
    }
  }, [merge, agentId]);

  const onPaste = useCallback(
    (e: ClipboardEvent) => {
      const files = imageFilesFrom(e.clipboardData).filter((f) => f.type.startsWith("image/"));
      if (files.length === 0) {
        // No image file from the webview. Plain text pastes normally; otherwise (empty, or only
        // file paths from a file manager) ask the OS clipboard directly.
        if (!hasText(e.clipboardData) || isFileListPaste(e.clipboardData)) {
          e.preventDefault();
          void addFromClipboard();
        }
        return;
      }
      // Image + text (e.g. a copied web snippet): keep the text paste, attach the image too.
      if (!hasText(e.clipboardData)) e.preventDefault();
      void add(files);
    },
    [add, addFromClipboard],
  );

  const dropHandlers = {
    onDragEnter: (e: DragEvent) => {
      if (!hasFiles(e.dataTransfer)) return;
      e.preventDefault();
      depth.current++;
      setDragging(true);
    },
    onDragOver: (e: DragEvent) => {
      if (!hasFiles(e.dataTransfer)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    },
    onDragLeave: (e: DragEvent) => {
      if (!hasFiles(e.dataTransfer)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) setDragging(false);
    },
    onDrop: (e: DragEvent) => {
      if (!hasFiles(e.dataTransfer)) return;
      e.preventDefault();
      depth.current = 0;
      setDragging(false);
      void add(imageFilesFrom(e.dataTransfer));
    },
  };

  /** Props for the hidden <input type="file">. */
  const inputProps = {
    ref: inputRef,
    type: "file" as const,
    accept: ACCEPTED_IMAGE_TYPES.join(","),
    multiple: true,
    hidden: true,
    tabIndex: -1,
    "aria-hidden": true,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => {
      void add(Array.from(e.target.files ?? []));
      e.target.value = "";
    },
  };

  return {
    attachments,
    setAttachments,
    add,
    merge,
    remove,
    clear,
    openPicker,
    onPaste,
    dropHandlers,
    dragging,
    inputProps,
  };
}

/**
 * One toast for what wasn't attached. `problems` are files that couldn't be used (format, size, unreadable);
 * `skipped` are pictures left out on purpose (already attached, over the limit).
 */
function reportProblems(problems: string[], skipped: string[], noun: "files" | "images") {
  const all = [...problems, ...skipped];
  if (all.length === 0) return;
  if (problems.length === 0) {
    // Nothing was wrong with the files themselves: no format reminder needed.
    toast.info(all.join(". "));
    return;
  }
  toast.warning(all.length === 1 ? all[0] : `${all.length} ${noun} weren't attached`, {
    description:
      all.length > 1
        ? all.join(" · ")
        : // Format problems already name the accepted formats.
          all[0].includes(ACCEPTED_IMAGE_LABEL)
          ? `Up to 5 MB each, ${MAX_ATTACHMENTS} per message.`
          : `${ACCEPTED_IMAGE_LABEL}, up to 5 MB each, ${MAX_ATTACHMENTS} per message.`,
  });
}
