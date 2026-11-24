import { describe, expect, it } from "vitest";
import { parseFileUri, parseInlineFileLine, resolveFileRefPath, toRelativeWorkspacePath } from "./fileLinks";

describe("parseFileUri", () => {
  it("parses unix and windows file URIs with optional line and range fragments", () => {
    expect(parseFileUri("file:///repo/src/app.ts#L42")).toEqual({
      path: "/repo/src/app.ts",
      line: 42,
      column: null,
      absolute: true,
    });
    expect(parseFileUri("file:///repo/src/my%20file.rs#L10-L25")).toEqual({
      path: "/repo/src/my file.rs",
      line: 10,
      column: null,
      absolute: true,
    });
    expect(parseFileUri("file:///C:/Users/me/app.ts#L7:3")).toEqual({
      path: "C:/Users/me/app.ts",
      line: 7,
      column: 3,
      absolute: true,
    });
    expect(parseFileUri("https://example.com/a.ts#L1")).toBeNull();
  });
});

describe("parseInlineFileLine", () => {
  it("parses relative and absolute file:line references", () => {
    expect(parseInlineFileLine("src/app.ts:42")).toEqual({
      path: "src/app.ts",
      line: 42,
      column: null,
      absolute: false,
    });
    expect(parseInlineFileLine("./crates/core/src/lib.rs:108:5")).toEqual({
      path: "crates/core/src/lib.rs",
      line: 108,
      column: 5,
      absolute: false,
    });
    expect(parseInlineFileLine("Cargo.toml:12")).toEqual({
      path: "Cargo.toml",
      line: 12,
      column: null,
      absolute: false,
    });
    expect(parseInlineFileLine("/abs/path/main.py:9")).toEqual({
      path: "/abs/path/main.py",
      line: 9,
      column: null,
      absolute: true,
    });
  });

  it("ignores URLs, host:port pairs, timestamps, and traversal", () => {
    expect(parseInlineFileLine("https://example.com:443")).toBeNull();
    expect(parseInlineFileLine("localhost:3000")).toBeNull();
    expect(parseInlineFileLine("127.0.0.1:8080")).toBeNull();
    expect(parseInlineFileLine("example.com:80")).toBeNull();
    expect(parseInlineFileLine("12:30")).toBeNull();
    expect(parseInlineFileLine("../secret.txt:1")).toBeNull();
    expect(parseInlineFileLine("foo:0")).toBeNull();
  });
});

describe("path resolution helpers", () => {
  it("joins relative paths and computes relative paths inside cwd", () => {
    const ref = parseInlineFileLine("src/main.rs:10")!;
    expect(resolveFileRefPath(ref, "/home/me/repo")).toBe("/home/me/repo/src/main.rs");
    expect(toRelativeWorkspacePath("/home/me/repo/src/main.rs", "/home/me/repo")).toBe("src/main.rs");
    expect(toRelativeWorkspacePath("/other/main.rs", "/home/me/repo")).toBeNull();
  });
});
