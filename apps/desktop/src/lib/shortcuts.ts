import { acceleratorFor } from "@/components/settings/HotkeyRecorder";
import { acceleratorCaps, sameAccelerator } from "@/components/settings/acceleratorLabels";
import { isMac } from "./utils";

export type ShortcutId =
  | "palette"
  | "search"
  | "newAgent"
  | "back"
  | "forward"
  | "needs"
  | "mission"
  | "voice"
  | "agentList"
  | "settings"
  | "help"
  | "workspace"
  | "preview"
  | "review"
  | "dictate"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset";

/** Every in-app shortcut you can change, with what it starts as. Stored as Tauri-style accelerators. */
export const SHORTCUTS: { id: ShortcutId; label: string; default: string }[] = [
  { id: "palette", label: "Command palette", default: "CmdOrCtrl+K" },
  { id: "search", label: "Search every conversation", default: "CmdOrCtrl+Shift+F" },
  { id: "newAgent", label: "New agent", default: "CmdOrCtrl+N" },
  { id: "back", label: "Back", default: isMac ? "CmdOrCtrl+[" : "Alt+Left" },
  { id: "forward", label: "Forward", default: isMac ? "CmdOrCtrl+]" : "Alt+Right" },
  { id: "needs", label: "Needs you", default: "CmdOrCtrl+J" },
  { id: "mission", label: "Mission Control: All agents", default: "CmdOrCtrl+Shift+M" },
  { id: "voice", label: "Voice assistant", default: "CmdOrCtrl+Shift+A" },
  { id: "dictate", label: "Dictate into the message box", default: "CmdOrCtrl+Shift+D" },
  { id: "agentList", label: "Show or hide the agent list", default: "CmdOrCtrl+B" },
  { id: "workspace", label: "Show or hide the workspace panel", default: "CmdOrCtrl+`" },
  { id: "preview", label: "Show or hide the web preview", default: "CmdOrCtrl+Shift+P" },
  { id: "review", label: "Review the agent's changes", default: "CmdOrCtrl+Shift+R" },
  { id: "settings", label: "Settings", default: "CmdOrCtrl+," },
  { id: "help", label: "Keyboard shortcuts", default: "CmdOrCtrl+/" },
  { id: "zoomIn", label: "Interface scale: larger", default: "CmdOrCtrl+=" },
  { id: "zoomOut", label: "Interface scale: smaller", default: "CmdOrCtrl+-" },
  { id: "zoomReset", label: "Reset interface scale", default: "CmdOrCtrl+0" },
];

/** Keys that stay as they are; a rebindable shortcut may not take them. */
const FIXED: { keys: string; label: string }[] = [
  ...Array.from({ length: 9 }, (_, i) => ({ keys: `CmdOrCtrl+${i + 1}`, label: "Jump to an agent" })),
  { keys: "CmdOrCtrl+F", label: "Find in this conversation" },
  { keys: "Alt+Up", label: "Previous agent" },
  { keys: "Alt+Down", label: "Next agent" },
  ...["A", "C", "V", "X", "Z", "Y"].map((k) => ({ keys: `CmdOrCtrl+${k}`, label: "a text editing key" })),
];

let overrides: Record<string, string> = {};

/** Called whenever the settings change. */
export function setShortcutOverrides(map: Record<string, string> | undefined) {
  overrides = map ?? {};
}

/** The accelerator an action uses now ("" = unassigned). */
export function bindingFor(id: ShortcutId, map: Record<string, string> = overrides): string {
  if (id in map) return map[id];
  return SHORTCUTS.find((s) => s.id === id)?.default ?? "";
}

/** The key caps to show for an action ([] when unassigned). */
export function capsFor(id: ShortcutId, map: Record<string, string> = overrides): string[] {
  return acceleratorCaps(bindingFor(id, map), isMac);
}

function modsOf(e: KeyboardEvent): string {
  const m: string[] = [];
  if (isMac ? e.metaKey : e.ctrlKey) m.push("CmdOrCtrl");
  if (isMac && e.ctrlKey) m.push("Ctrl");
  if (!isMac && e.metaKey) m.push("Super");
  if (e.altKey) m.push("Alt");
  if (e.shiftKey) m.push("Shift");
  return m.join("+");
}

/** Does this key press trigger the action? By key position, or by the character typed (other layouts). */
export function matchesShortcut(e: KeyboardEvent, id: ShortcutId, map: Record<string, string> = overrides): boolean {
  const b = bindingFor(id, map);
  if (!b) return false;
  if (acceleratorFor(e) === b) return true;
  const parts = b.split("+");
  const key = parts.pop() ?? "";
  const mods = parts.join("+");
  if (key.length !== 1) return false;
  // "+" is Shift and "=" on most keyboards: bound to "=", both count.
  if (key === "=" && e.key === "+") return modsOf(e).replace("+Shift", "").replace("Shift", "") === mods;
  return e.key.toLowerCase() === key.toLowerCase() && modsOf(e) === mods;
}

/** The first action the key press triggers, or null. */
export function shortcutFor(e: KeyboardEvent): ShortcutId | null {
  return SHORTCUTS.find((s) => matchesShortcut(e, s.id))?.id ?? null;
}

/** A plain sentence when `accelerator` can't be used for `id`, else null. */
export function shortcutConflict(
  id: ShortcutId,
  accelerator: string,
  map: Record<string, string>,
  taken: { keys: string; label: string }[] = [],
): string | null {
  const same = (a: string, b: string) => sameAccelerator(a, b, isMac);
  const other = SHORTCUTS.find((s) => s.id !== id && same(bindingFor(s.id, map), accelerator));
  if (other) return `Already used for "${other.label}". Change or unassign that one first.`;
  const fixed = [...FIXED, ...taken].find((f) => same(f.keys, accelerator));
  if (fixed) return `Kvistcode already uses this for ${fixed.label}.`;
  return null;
}
