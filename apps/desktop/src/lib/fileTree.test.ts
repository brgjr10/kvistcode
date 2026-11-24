import { describe, expect, it } from "vitest";
import { ancestorDirs, buildFileTree } from "./fileTree";

describe("buildFileTree", () => {
  it("builds a directories-first sorted tree", () => {
    const tree = buildFileTree(["README.md", "src/z.ts", "src/a.ts", "apps/web/index.html"]);
    expect(tree.map((n) => `${n.kind}:${n.name}`)).toEqual(["dir:apps", "dir:src", "file:README.md"]);
    expect(tree[1].children.map((n) => n.name)).toEqual(["a.ts", "z.ts"]);
  });

  it("filters by multi-term query across path segments", () => {
    const tree = buildFileTree(["src/app.ts", "src/components/Button.tsx", "docs/guide.md"], "comp btn");
    expect(tree).toEqual([]);
    const match = buildFileTree(["src/app.ts", "src/components/Button.tsx", "docs/guide.md"], "comp button");
    expect(match).toHaveLength(1);
    expect(match[0].children[0].children[0].path).toBe("src/components/Button.tsx");
  });

  it("computes ancestor directories", () => {
    expect(ancestorDirs("a/b/c.ts")).toEqual(["a", "a/b"]);
    expect(ancestorDirs("README.md")).toEqual([]);
  });
});
