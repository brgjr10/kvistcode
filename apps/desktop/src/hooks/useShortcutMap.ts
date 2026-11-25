import { useApp } from "@/store/app";
import { capsFor, setShortcutOverrides, type ShortcutId } from "@/lib/shortcuts";

// The key handlers read the current bindings from `lib/shortcuts`; keep them in step with the settings.
setShortcutOverrides(useApp.getState().settings?.shortcuts);
useApp.subscribe((s, prev) => {
  if (s.settings?.shortcuts !== prev.settings?.shortcuts) setShortcutOverrides(s.settings?.shortcuts);
});

/** The user's shortcut changes, for views that show the keys (they re-render when a binding changes). */
export function useShortcutMap(): Record<string, string> {
  return useApp((s) => s.settings?.shortcuts) ?? NONE;
}
const NONE: Record<string, string> = {};

/** The key caps for one action, or undefined while it has none. */
export function useShortcutCaps(id: ShortcutId): string[] | undefined {
  const map = useShortcutMap();
  const caps = capsFor(id, map);
  return caps.length > 0 ? caps : undefined;
}
