import { describe, expect, it } from "vitest";
import { conversationKey, findCountLabel, holdsFindTarget } from "./findInConversation";

const key = (k: string, mods: Partial<Record<"ctrlKey" | "metaKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("conversationKey", () => {
  it("opens Find with Ctrl+F, or ⌘F on a Mac", () => {
    expect(conversationKey(key("f", { ctrlKey: true }), false, false)).toBe("find");
    expect(conversationKey(key("F", { ctrlKey: true }), false, true)).toBe("find");
    expect(conversationKey(key("f", { metaKey: true }), true, false)).toBe("find");
  });

  it("leaves Ctrl+F alone on a Mac (forward a character) and ⌘F alone elsewhere", () => {
    expect(conversationKey(key("f", { ctrlKey: true }), true, true)).toBeNull();
    expect(conversationKey(key("f", { metaKey: true }), false, false)).toBeNull();
  });

  it("leaves Ctrl+Shift+F (search every conversation) to the palette", () => {
    expect(conversationKey(key("F", { ctrlKey: true, shiftKey: true }), false, false)).toBeNull();
  });

  it("steps between your messages with Alt+Shift+Up / Down", () => {
    expect(conversationKey(key("ArrowUp", { altKey: true, shiftKey: true }), false, false)).toBe("prev-message");
    expect(conversationKey(key("ArrowDown", { altKey: true, shiftKey: true }), false, true)).toBe("next-message");
    expect(conversationKey(key("ArrowUp", { altKey: true, shiftKey: true }), true, false)).toBe("prev-message");
  });

  it("keeps ⌥⇧↑ for selecting text in a Mac text field, and plain Alt+Up for switching agents", () => {
    expect(conversationKey(key("ArrowUp", { altKey: true, shiftKey: true }), true, true)).toBeNull();
    expect(conversationKey(key("ArrowUp", { altKey: true }), false, false)).toBeNull();
  });
});

describe("holdsFindTarget", () => {
  it("is the item itself or a piece of its run of messages", () => {
    expect(holdsFindTarget("a1", "a1")).toBe(true);
    expect(holdsFindTarget("a2", "a1", [{ id: "a1" }, { id: "a2" }])).toBe(true);
    expect(holdsFindTarget("a3", "a1", [{ id: "a1" }, { id: "a2" }])).toBe(false);
  });

  it("holds nothing while Find is closed", () => {
    expect(holdsFindTarget(null, "a1")).toBe(false);
  });
});

describe("findCountLabel", () => {
  it("says where you are among the matches", () => {
    expect(findCountLabel("webhook", 1, 5)).toBe("2 of 5");
    expect(findCountLabel("webhook", -1, 0)).toBe("No matches");
    expect(findCountLabel("   ", 0, 3)).toBe("");
  });

  it("marks the count as a floor while earlier messages are not loaded", () => {
    expect(findCountLabel("webhook", 0, 3, true)).toBe("1 of 3+");
    expect(findCountLabel("webhook", -1, 0, true)).toBe("None loaded");
  });

  it("never shows a position past the count while a new query settles", () => {
    expect(findCountLabel("web", 4, 2)).toBe("– of 2");
    expect(findCountLabel("web", -1, 2)).toBe("– of 2");
  });
});
