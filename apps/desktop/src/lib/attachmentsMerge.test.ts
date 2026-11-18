import { describe, expect, it } from "vitest";
import { MAX_ATTACHMENTS, mergeAttachments, skippedNotes } from "./attachments";
import type { Attachment } from "./types";

const img = (data: string, name = `${data}.png`, mimeType = "image/png"): Attachment => ({ name, mimeType, data });

describe("mergeAttachments", () => {
  it("adds new pictures after the ones already attached", () => {
    const r = mergeAttachments([img("a")], [img("b"), img("c")]);
    expect(r.next.map((a) => a.data)).toEqual(["a", "b", "c"]);
    expect(r.duplicates).toBe(0);
    expect(r.dropped).toBe(0);
  });

  it("skips a picture that is already attached, whatever its name", () => {
    const r = mergeAttachments([img("a", "shot.png")], [img("a", "pasted-image.png"), img("b")]);
    expect(r.next.map((a) => a.data)).toEqual(["a", "b"]);
    expect(r.duplicates).toBe(1);
  });

  it("skips the same picture given twice at once", () => {
    const r = mergeAttachments([], [img("a"), img("a")]);
    expect(r.next).toHaveLength(1);
    expect(r.duplicates).toBe(1);
  });

  it("treats the same bytes in another format as a different picture", () => {
    const r = mergeAttachments([img("a")], [img("a", "a.jpg", "image/jpeg")]);
    expect(r.next).toHaveLength(2);
  });

  it("compares pictures that point at a file by their path", () => {
    const file = (path: string): Attachment => ({ name: "x.png", mimeType: "image/png", path });
    const r = mergeAttachments([file("/tmp/a.png")], [file("/tmp/a.png"), file("/tmp/b.png")]);
    expect(r.next.map((a) => a.path)).toEqual(["/tmp/a.png", "/tmp/b.png"]);
    expect(r.duplicates).toBe(1);
  });

  it("keeps within the per-message limit and counts the rest", () => {
    const current = Array.from({ length: MAX_ATTACHMENTS - 1 }, (_, i) => img(`c${i}`));
    const r = mergeAttachments(current, [img("x"), img("y"), img("z")]);
    expect(r.next).toHaveLength(MAX_ATTACHMENTS);
    expect(r.next[MAX_ATTACHMENTS - 1].data).toBe("x");
    expect(r.dropped).toBe(2);
  });

  it("a repeat doesn't use up room", () => {
    const current = Array.from({ length: MAX_ATTACHMENTS - 1 }, (_, i) => img(`c${i}`));
    const r = mergeAttachments(current, [img("c0"), img("new")]);
    expect(r.next).toHaveLength(MAX_ATTACHMENTS);
    expect(r.duplicates).toBe(1);
    expect(r.dropped).toBe(0);
  });

  it("leaves the current list untouched", () => {
    const current = [img("a")];
    mergeAttachments(current, [img("b")]);
    expect(current).toHaveLength(1);
  });
});

describe("skippedNotes", () => {
  it("says nothing when every picture went in", () => {
    expect(skippedNotes(0, 0)).toEqual([]);
  });
  it("names repeats and the limit separately", () => {
    expect(skippedNotes(1, 0)).toEqual(["That image is already attached"]);
    expect(skippedNotes(2, 0)).toEqual(["2 images were already attached"]);
    expect(skippedNotes(0, 3)).toEqual([`Only ${MAX_ATTACHMENTS} images per message; 3 not attached`]);
    expect(skippedNotes(1, 1)).toHaveLength(2);
  });
});
