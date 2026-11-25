import { describe, expect, it } from "vitest";
import {
  caretOnFirstLine,
  caretOnLastLine,
  isBigPaste,
  quoteBlock,
  sentHistory,
  sizeLabel,
  withPastes,
} from "./composerExtras";
import type { TimelineItem } from "./types";

const user = (text: string, extra: object = {}) =>
  ({ kind: "user_message", text, ...extra }) as unknown as TimelineItem;

describe("big pastes", () => {
  it("only very large or very long text counts", () => {
    expect(isBigPaste("hello\n".repeat(50))).toBe(false);
    expect(isBigPaste("x".repeat(40_000))).toBe(true);
    expect(isBigPaste("a\n".repeat(700))).toBe(true);
  });
  it("is appended as a fenced block that its own backticks cannot close", () => {
    expect(withPastes("look", [{ id: 1, text: "a\nb\n" }])).toBe("look\n\n```\na\nb\n```");
    expect(withPastes("", [{ id: 1, text: "```js\nx\n```" }])).toBe("````\n```js\nx\n```\n````");
  });
  it("labels sizes", () => {
    expect(sizeLabel(40_000)).toBe("39 KB");
    expect(sizeLabel(3 * 1024 * 1024)).toBe("3.0 MB");
  });
});

describe("sent history", () => {
  it("keeps your messages in order, without automatic ones or repeats", () => {
    const h = sentHistory([user("a"), user("a"), user("auto", { automatic: true }), user("b"), user("  ")]);
    expect(h).toEqual(["a", "b"]);
  });
});

describe("quote and caret", () => {
  it("quotes every line", () => {
    expect(quoteBlock("one\n\ntwo")).toBe("> one\n>\n> two\n\n");
  });
  it("finds the first and last line", () => {
    expect(caretOnFirstLine("ab\ncd", 1, 1)).toBe(true);
    expect(caretOnFirstLine("ab\ncd", 4, 4)).toBe(false);
    expect(caretOnLastLine("ab\ncd", 4, 4)).toBe(true);
    expect(caretOnLastLine("ab\ncd", 1, 1)).toBe(false);
    expect(caretOnLastLine("ab", 0, 2)).toBe(false);
  });
});
