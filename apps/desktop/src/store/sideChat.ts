import { create } from "zustand";
import type { SideChatEntry } from "@/lib/types";
import { onAgentRemoved } from "./app";

interface SideChatStore {
  /** Ephemeral `/btw` side-chat Q&A entries per agent, oldest first. */
  byAgent: Record<string, SideChatEntry[]>;
  /** Collapsed state per agent's side-chat card (defaults to expanded when entries arrive). */
  collapsed: Record<string, boolean>;
  upsert: (entry: SideChatEntry) => void;
  dismiss: (agentId: string, entryId: string) => void;
  clear: (agentId: string) => void;
  toggleCollapsed: (agentId: string) => void;
}

const MAX_PER_AGENT = 10;

export const useSideChat = create<SideChatStore>((set) => ({
  byAgent: {},
  collapsed: {},
  upsert: (entry) =>
    set((s) => {
      const list = s.byAgent[entry.agentId] ?? [];
      const idx = list.findIndex((x) => x.id === entry.id);
      const next = idx >= 0 ? list.map((x, i) => (i === idx ? entry : x)) : [...list, entry].slice(-MAX_PER_AGENT);
      const collapsed = idx < 0 ? { ...s.collapsed, [entry.agentId]: false } : s.collapsed;
      return {
        byAgent: { ...s.byAgent, [entry.agentId]: next },
        collapsed,
      };
    }),
  dismiss: (agentId, entryId) =>
    set((s) => {
      const list = (s.byAgent[agentId] ?? []).filter((x) => x.id !== entryId);
      if (list.length === 0) {
        const { [agentId]: _gone, ...byAgent } = s.byAgent;
        return { byAgent };
      }
      return { byAgent: { ...s.byAgent, [agentId]: list } };
    }),
  clear: (agentId) =>
    set((s) => {
      if (!(agentId in s.byAgent)) return s;
      const { [agentId]: _gone, ...byAgent } = s.byAgent;
      const { [agentId]: _c, ...collapsed } = s.collapsed;
      return { byAgent, collapsed };
    }),
  toggleCollapsed: (agentId) =>
    set((s) => ({ collapsed: { ...s.collapsed, [agentId]: !s.collapsed[agentId] } })),
}));

onAgentRemoved((id) => useSideChat.getState().clear(id));
