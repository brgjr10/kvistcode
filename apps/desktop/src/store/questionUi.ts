import { create } from "zustand";
import { onAgentRemoved, useApp } from "./app";
import { EMPTY_DRAFT, type QuestionDraft } from "@/lib/question";

/**
 * Per-request state of question cards that must survive the row being virtualized away:
 * what was typed/picked so far, and whether the card is on screen (keys only act on visible ones).
 */
interface QuestionUiStore {
  drafts: Record<string, QuestionDraft>;
  visible: Record<string, boolean>;
  /** The pinned dock answers this question with the number keys: the card in the timeline leaves them alone. */
  docked: Record<string, boolean>;
  /** The pinned dock shows this question (in any shape): the card in the timeline folds to one line. */
  inDock: Record<string, boolean>;
  update: (requestId: string, fn: (d: QuestionDraft) => QuestionDraft) => void;
  setVisible: (requestId: string, visible: boolean) => void;
  setDocked: (requestId: string, docked: boolean) => void;
  setInDock: (requestId: string, inDock: boolean) => void;
  /** Forget a resolved request. */
  clear: (requestId: string) => void;
}

export const useQuestionUi = create<QuestionUiStore>((set) => ({
  drafts: {},
  visible: {},
  docked: {},
  inDock: {},
  update: (id, fn) => set((s) => ({ drafts: { ...s.drafts, [id]: fn(s.drafts[id] ?? EMPTY_DRAFT) } })),
  setVisible: (id, v) => set((s) => (s.visible[id] === v ? s : { visible: { ...s.visible, [id]: v } })),
  setDocked: (id, v) =>
    set((s) => {
      if (Boolean(s.docked[id]) === v) return s;
      const docked = { ...s.docked };
      if (v) docked[id] = true;
      else delete docked[id];
      return { docked };
    }),
  setInDock: (id, v) =>
    set((s) => {
      if (Boolean(s.inDock[id]) === v) return s;
      const inDock = { ...s.inDock };
      if (v) inDock[id] = true;
      else delete inDock[id];
      return { inDock };
    }),
  clear: (id) =>
    set((s) => {
      if (!(id in s.drafts) && !(id in s.visible) && !(id in s.docked) && !(id in s.inDock)) return s;
      const drafts = { ...s.drafts };
      const visible = { ...s.visible };
      const docked = { ...s.docked };
      const inDock = { ...s.inDock };
      delete drafts[id];
      delete visible[id];
      delete docked[id];
      delete inDock[id];
      return { drafts, visible, docked, inDock };
    }),
}));

// An agent deleted for real leaves no question card behind: forget what was kept for its requests.
onAgentRemoved((agentId) => {
  for (const it of useApp.getState().items[agentId] ?? []) {
    if (it.kind === "question") useQuestionUi.getState().clear(it.requestId);
  }
});
