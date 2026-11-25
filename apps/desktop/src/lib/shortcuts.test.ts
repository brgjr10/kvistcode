import { describe, expect, it } from "vitest";
import { isMac } from "./utils";
import { bindingFor, capsFor, matchesShortcut, SHORTCUTS, shortcutConflict } from "./shortcuts";

const press = (key: string, code: string, mods: Partial<KeyboardEvent> = {}) =>
  ({
    key,
    code,
    ctrlKey: !isMac,
    metaKey: isMac,
    altKey: false,
    shiftKey: false,
    ...mods,
  }) as KeyboardEvent;

describe("shortcut bindings", () => {
  it("start as the defaults, and a change or an empty value replaces them", () => {
    expect(bindingFor("palette", {})).toBe("CmdOrCtrl+K");
    expect(bindingFor("palette", { palette: "CmdOrCtrl+P" })).toBe("CmdOrCtrl+P");
    expect(bindingFor("palette", { palette: "" })).toBe("");
    expect(capsFor("palette", { palette: "" })).toEqual([]);
  });

  it("match the key pressed, with exactly the modifiers bound", () => {
    expect(matchesShortcut(press("k", "KeyK"), "palette", {})).toBe(true);
    expect(matchesShortcut(press("K", "KeyK", { shiftKey: true }), "palette", {})).toBe(false);
    expect(matchesShortcut(press("k", "KeyK"), "palette", { palette: "CmdOrCtrl+P" })).toBe(false);
    expect(matchesShortcut(press("p", "KeyP"), "palette", { palette: "CmdOrCtrl+P" })).toBe(true);
    expect(matchesShortcut(press("k", "KeyK"), "palette", { palette: "" })).toBe(false);
  });

  it("zoom in answers to + as well as =", () => {
    expect(matchesShortcut(press("+", "Equal", { shiftKey: true }), "zoomIn", {})).toBe(true);
    expect(matchesShortcut(press("=", "Equal"), "zoomIn", {})).toBe(true);
  });

  it("warn about another action, a fixed key or a global hotkey, and allow free keys", () => {
    expect(shortcutConflict("help", "CmdOrCtrl+K", {}, [])).toMatch(/Command palette/);
    expect(shortcutConflict("help", "CmdOrCtrl+2", {}, [])).toMatch(/Jump to an agent/);
    expect(
      shortcutConflict("help", "CmdOrCtrl+Shift+Space", {}, [
        { keys: "CmdOrCtrl+Shift+Space", label: "the global hotkey" },
      ]),
    ).toMatch(/global hotkey/);
    expect(shortcutConflict("help", "CmdOrCtrl+Shift+U", {}, [])).toBeNull();
    // Unassigning the owner frees its keys.
    expect(shortcutConflict("help", "CmdOrCtrl+K", { palette: "" }, [])).toBeNull();
    expect(new Set(SHORTCUTS.map((s) => s.id)).size).toBe(SHORTCUTS.length);
  });
});
