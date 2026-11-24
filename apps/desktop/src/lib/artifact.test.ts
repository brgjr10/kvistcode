import { describe, expect, it } from "vitest";
import {
  ARTIFACT_CSP,
  ARTIFACT_MAX_BYTES,
  ARTIFACT_MAX_HEIGHT,
  ARTIFACT_MIN_HEIGHT,
  artifactKind,
  artifactSrc,
  buildArtifactDoc,
  buildSvgDoc,
  parseHeightMessage,
  previewByDefault,
  withinCap,
} from "./artifact";

describe("artifactKind", () => {
  it("detects tagged blocks", () => {
    expect(artifactKind("html", "<b>x</b>")).toBe("html");
    expect(artifactKind("HTML", "<b>x</b>")).toBe("html");
    expect(artifactKind("svg", "<svg viewBox='0 0 1 1'></svg>")).toBe("svg");
    expect(artifactKind("svg", "not svg")).toBeNull();
    expect(artifactKind("html", "  ")).toBeNull();
  });
  it("detects untagged whole documents", () => {
    expect(artifactKind(null, "<!DOCTYPE html><html></html>")).toBe("html");
    expect(artifactKind("text", "<svg></svg>")).toBe("svg");
    expect(artifactKind(null, "<div>x</div>")).toBeNull();
    expect(artifactKind("js", "<!doctype html>")).toBeNull();
  });
  it("opens whole documents on preview only", () => {
    expect(previewByDefault("html", "<!doctype html><p>x")).toBe(true);
    expect(previewByDefault("html", "<p>x</p>")).toBe(false);
    expect(previewByDefault("svg", "<svg></svg>")).toBe(false);
  });
});

describe("size cap", () => {
  it("allows up to the cap and refuses above", () => {
    expect(withinCap("a".repeat(ARTIFACT_MAX_BYTES))).toBe(true);
    expect(withinCap("a".repeat(ARTIFACT_MAX_BYTES + 1))).toBe(false);
  });
  it("counts bytes, not characters", () => {
    expect(withinCap("€".repeat(ARTIFACT_MAX_BYTES / 3))).toBe(true);
    expect(withinCap("€".repeat(Math.floor(ARTIFACT_MAX_BYTES / 3) + 1))).toBe(false);
  });
});

describe("document builder", () => {
  it("denies the network and puts the CSP before agent markup", () => {
    expect(ARTIFACT_CSP).toContain("default-src 'none'");
    expect(ARTIFACT_CSP).not.toMatch(/https?:|connect-src/);
    const doc = buildArtifactDoc("<p>hi</p><script>fetch('x')</script>", "id1");
    expect(doc.indexOf("Content-Security-Policy")).toBeLessThan(doc.indexOf("<p>hi</p>"));
  });
  it("injects into an existing head or html", () => {
    const a = buildArtifactDoc("<!doctype html><html><head><title>t</title></head><body>b</body></html>", "i");
    expect(a.indexOf("Content-Security-Policy")).toBeLessThan(a.indexOf("<title>"));
    const b = buildArtifactDoc("<html><body>b</body></html>", "i");
    expect(b).toMatch(/<html><head><meta/);
    expect(b.indexOf("Content-Security-Policy")).toBeLessThan(b.indexOf("<body>"));
  });
  it("embeds the id safely", () => {
    expect(buildArtifactDoc("x", 'a"</script>')).not.toContain("</script>\"");
  });
  it("shows svg through an img", () => {
    const doc = buildSvgDoc("<svg><script>alert(1)</script></svg>", "i");
    expect(doc).toContain('<img alt="" src="data:image/svg+xml;base64,');
    expect(doc).not.toContain("alert(1)");
  });
  it("makes a data url", () => {
    expect(artifactSrc("html", "<p>é</p>", "i")).toMatch(/^data:text\/html;base64,/);
  });
});

describe("parseHeightMessage", () => {
  const win = {};
  const ok = { kvArtifact: 1, id: "a", height: 300 };
  it("accepts the exact shape from the frame", () => {
    expect(parseHeightMessage({ source: win, data: ok }, win, "a")).toBe(300);
  });
  it("clamps", () => {
    expect(parseHeightMessage({ source: win, data: { ...ok, height: 99999 } }, win, "a")).toBe(ARTIFACT_MAX_HEIGHT);
    expect(parseHeightMessage({ source: win, data: { ...ok, height: 0 } }, win, "a")).toBe(ARTIFACT_MIN_HEIGHT);
  });
  it("rejects other sources, ids and shapes", () => {
    expect(parseHeightMessage({ source: {}, data: ok }, win, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: ok }, null, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: ok }, win, "b")).toBeNull();
    expect(parseHeightMessage({ source: win, data: { ...ok, extra: 1 } }, win, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: { ...ok, height: "9" } }, win, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: { ...ok, height: NaN } }, win, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: { ...ok, height: -1 } }, win, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: null }, win, "a")).toBeNull();
    expect(parseHeightMessage({ source: win, data: [1, 2, 3] }, win, "a")).toBeNull();
  });
});
