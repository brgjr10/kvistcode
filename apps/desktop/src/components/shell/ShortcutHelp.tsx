import { create } from "zustand";
import { APPROVAL_KEYS } from "@/lib/approvalKeys";
import { useShortcutMap } from "@/hooks/useShortcutMap";
import { capsFor, type ShortcutId } from "@/lib/shortcuts";
import { isMac, modKey } from "@/lib/utils";
import { Dialog, DialogBody, DialogContent } from "../ui/dialog";
import { Kbd } from "../ui/kbd";

const useHelp = create<{ open: boolean; setOpen: (o: boolean) => void }>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));

export function openShortcutHelp() {
  useHelp.getState().setOpen(true);
}

export function toggleShortcutHelp() {
  const s = useHelp.getState();
  s.setOpen(!s.open);
}

type Row = { keys: string[]; label: string; id?: ShortcutId };

const SECTIONS: { title: string; rows: Row[] }[] = [
  {
    title: "Anywhere",
    rows: [
      { keys: [modKey, "K"], label: "Command palette (type > to start an agent)", id: "palette" },
      { keys: [modKey, "⇧", "F"], label: "Search every conversation (type ? in the palette)", id: "search" },
      { keys: [modKey, "N"], label: "New agent", id: "newAgent" },
      { keys: isMac ? ["⌘", "["] : ["Alt", "←"], label: "Back (also the mouse's back button)", id: "back" },
      { keys: isMac ? ["⌘", "]"] : ["Alt", "→"], label: "Forward", id: "forward" },
      { keys: [modKey, "J"], label: "Needs you (press again for the next one)", id: "needs" },
      { keys: [modKey, "⇧", "M"], label: "Mission Control: All agents", id: "mission" },
      { keys: [modKey, "⇧", "A"], label: "Voice assistant: start, end or unmute (Esc ends)", id: "voice" },
      { keys: [modKey, "B"], label: "Show or hide the agent list", id: "agentList" },
      { keys: [modKey, "1–9"], label: "Jump to an agent" },
      { keys: isMac ? ["⌥", "↑", "↓"] : ["Alt", "↑", "↓"], label: "Previous or next agent (not while typing)" },
      { keys: [modKey, ","], label: "Settings", id: "settings" },
      { keys: [modKey, "+"], label: "Interface scale: larger", id: "zoomIn" },
      { keys: [modKey, "−"], label: "Interface scale: smaller", id: "zoomOut" },
      { keys: [modKey, "0"], label: "Reset interface scale", id: "zoomReset" },
      { keys: [modKey, "/"], label: "This cheat sheet (also ?)", id: "help" },
      { keys: ["Esc"], label: "Close the open dialog or menu" },
    ],
  },
  {
    title: "Approvals (agent view, Mission Control)",
    rows: [...APPROVAL_KEYS, { keys: ["⏎"], label: "Allow once, when focus is on the approval card" }],
  },
  {
    title: "Agent view",
    rows: [
      { keys: ["⏎"], label: "Send (while the agent works: queue, start a helper, or ask, per Settings)" },
      { keys: [modKey, "⏎"], label: "While the agent works: start a helper agent with this message" },
      { keys: [modKey, isMac ? "⌥" : "Alt", "←/→"], label: "Move focus between tiled helper agents" },
      { keys: ["⇧", "⏎"], label: "New line" },
      {
        keys: [modKey, "⇧", "D"],
        label: "Dictate: start or stop (⏎ stops and transcribes, Esc cancels)",
        id: "dictate",
      },
      { keys: ["/"], label: "Slash commands (at the start of the message)" },
      { keys: ["@"], label: "Mention a file" },
      { keys: ["Esc"], label: "Stop this turn (press twice from an empty composer)" },
      { keys: [modKey, "F"], label: "Find in this conversation (⏎ next, ⇧⏎ previous)" },
      { keys: isMac ? ["⌥", "⇧", "↑", "↓"] : ["Alt", "⇧", "↑", "↓"], label: "Previous or next message you sent" },
      { keys: [modKey, "`"], label: "Show or hide the workspace panel (terminals, scripts)", id: "workspace" },
      { keys: [modKey, "⇧", "P"], label: "Show or hide the web preview beside the conversation", id: "preview" },
      { keys: [modKey, "⇧", "R"], label: "Review the agent's changes (agents with their own worktree)", id: "review" },
      { keys: ["F2"], label: "Rename the focused agent in the list" },
      { keys: ["↑", "↓"], label: "Move between agents in the list (Home and End too)" },
      { keys: isMac ? ["⌥", "↑", "↓"] : ["Alt", "↑", "↓"], label: "Move the focused project up or down" },
      { keys: ["/"], label: "Filter the agent list (when nothing is focused)" },
    ],
  },
  {
    title: "Mission Control: Needs you",
    rows: [
      { keys: ["J"], label: "Next" },
      { keys: ["K"], label: "Previous" },
      { keys: ["⏎"], label: "Open the agent" },
      { keys: ["E"], label: "Done (mark it handled)" },
      { keys: ["⇧", "E"], label: "Archive" },
    ],
  },
  {
    title: "Starting agents",
    rows: [
      { keys: ["⏎"], label: "Home: start the agent (⇧⏎ for a new line)" },
      { keys: [modKey, "⏎"], label: "New agent: launch, from any field" },
      {
        keys: [isMac ? "⌥" : "Alt", "1–6"],
        label: "New agent: pick Claude, Codex, Grok, Gemini, Local or a custom agent",
      },
      { keys: [modKey, "⏎"], label: "Command palette: start an agent with what you typed" },
    ],
  },
  {
    title: "Review",
    rows: [
      { keys: ["↑", "↓"], label: "Move between diff lines (j / k too)" },
      { keys: ["⏎"], label: "Comment on the line (or c)" },
      { keys: [modKey, "⏎"], label: "Save the comment" },
    ],
  },
];

/** "?" / Ctrl+/ overlay listing every keyboard shortcut. */
export function ShortcutHelp() {
  const open = useHelp((s) => s.open);
  const setOpen = useHelp((s) => s.setOpen);
  const map = useShortcutMap();
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        title="Keyboard shortcuts"
        description="Kvistcode is fully usable from the keyboard."
        className="w-[min(760px,calc(100vw-16px))]"
        sheet
      >
        <DialogBody className="grid gap-x-8 gap-y-5 pb-5 sm:grid-cols-2">
          {SECTIONS.map((sec) => (
            <section key={sec.title} aria-labelledby={`sc-${sec.title}`}>
              <h3 id={`sc-${sec.title}`} className="mb-2 eyebrow text-steel">
                {sec.title}
              </h3>
              <dl className="space-y-1.5">
                {sec.rows.map((r) => {
                  const keys = r.id ? capsFor(r.id, map) : r.keys;
                  return (
                    <div key={r.label} className="flex items-center justify-between gap-3 text-small">
                      <dt className="text-muted">{r.label}</dt>
                      <dd className="flex shrink-0 gap-0.5">
                        {keys.length > 0 ? (
                          keys.map((k) => <Kbd key={k}>{k}</Kbd>)
                        ) : (
                          <span className="text-steel">Not set</span>
                        )}
                      </dd>
                    </div>
                  );
                })}
              </dl>
            </section>
          ))}
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
