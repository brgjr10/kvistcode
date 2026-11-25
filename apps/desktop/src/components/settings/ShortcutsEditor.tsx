import { RotateCcw, X } from "lucide-react";
import { useState } from "react";
import { bindingFor, capsFor, SHORTCUTS, shortcutConflict, type ShortcutId } from "@/lib/shortcuts";
import type { Settings } from "@/lib/types";
import { errorMessage, isMac, modKey } from "@/lib/utils";
import { useApp } from "@/store/app";
import { Button } from "../ui/button";
import { Kbd } from "../ui/kbd";
import { HotkeyRecorder } from "./HotkeyRecorder";

const FIXED_ROWS: { keys: string[]; label: string }[] = [
  { keys: [modKey, "1–9"], label: "Jump to an agent" },
  { keys: [modKey, "F"], label: "Find in this conversation" },
  { keys: [isMac ? "⌥" : "Alt", "↑", "↓"], label: "Previous or next agent" },
  { keys: ["Esc"], label: "Close, or stop the turn" },
];

/** Settings → Keyboard: every in-app shortcut, with change, reset and unassign. */
export function ShortcutsEditor({ settings }: { settings: Settings }) {
  const map = settings.shortcuts ?? {};
  const [warning, setWarning] = useState<{ id: ShortcutId; text: string } | null>(null);

  const save = async (next: Record<string, string>) => {
    try {
      await useApp.getState().patchSettings({ shortcuts: next });
    } catch (e) {
      setWarning({ id: "palette", text: `Couldn't save: ${errorMessage(e)}` });
    }
  };
  const set = (id: ShortcutId, accelerator: string) => {
    const def = SHORTCUTS.find((s) => s.id === id)?.default ?? "";
    const next = { ...map };
    if (accelerator === def) delete next[id];
    else next[id] = accelerator;
    void save(next);
  };
  const change = (id: ShortcutId, accelerator: string) => {
    const taken = [
      { keys: settings.globalHotkey, label: "the global hotkey" },
      { keys: settings.voiceAssistant.hotkey, label: "the voice assistant's global hotkey" },
    ].filter((t) => t.keys.trim());
    const problem = shortcutConflict(id, accelerator, map, taken);
    if (problem) {
      setWarning({ id, text: problem });
      return;
    }
    setWarning(null);
    set(id, accelerator);
  };

  return (
    <>
      {SHORTCUTS.map((s) => {
        const caps = capsFor(s.id, map);
        const changed = s.id in map;
        return (
          <div key={s.id} className="px-4 py-2.5">
            <div className="flex items-center justify-between gap-3">
              <span className="min-w-0 text-body text-muted">{s.label}</span>
              <span className="flex shrink-0 items-center gap-2">
                <span className="flex gap-1">
                  {caps.length > 0 ? (
                    caps.map((k) => <Kbd key={k}>{k}</Kbd>)
                  ) : (
                    <span className="text-meta text-steel">Not set</span>
                  )}
                </span>
                <HotkeyRecorder label="Change" onRecord={(acc) => change(s.id, acc)} />
                {changed && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Reset ${s.label} to ${SHORTCUTS.find((d) => d.id === s.id)?.default}`}
                    onClick={() => {
                      setWarning(null);
                      set(s.id, s.default);
                    }}
                  >
                    <RotateCcw /> Reset
                  </Button>
                )}
                {bindingFor(s.id, map) !== "" && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Unassign ${s.label}`}
                    onClick={() => {
                      setWarning(null);
                      const next = { ...map, [s.id]: "" };
                      void save(next);
                    }}
                  >
                    <X /> Unassign
                  </Button>
                )}
              </span>
            </div>
            {warning?.id === s.id && (
              <p role="alert" className="mt-1.5 text-meta leading-snug text-err">
                {warning.text}
              </p>
            )}
          </div>
        );
      })}
      {FIXED_ROWS.map((r) => (
        <div key={r.label} className="flex items-center justify-between gap-3 px-4 py-2.5">
          <span className="min-w-0 text-body text-muted">{r.label}</span>
          <span className="flex shrink-0 items-center gap-2">
            <span className="text-meta text-steel">Fixed</span>
            <span className="flex gap-1">
              {r.keys.map((k) => (
                <Kbd key={k}>{k}</Kbd>
              ))}
            </span>
          </span>
        </div>
      ))}
    </>
  );
}
