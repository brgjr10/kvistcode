import { describe, expect, it } from "vitest";
import { plainPreview } from "./preview";

describe("plainPreview", () => {
  it("drops markdown marks, links and code blocks", () => {
    expect(plainPreview("## Done\n\n**All 14 specs pass.** See [the PR](https://x.test/1).")).toBe(
      "Done All 14 specs pass. See the PR.",
    );
    expect(plainPreview("Before\n```ts\nconst a = 1;\n```\nafter")).toBe("Before after");
    expect(plainPreview("Run `npm test` now")).toBe("Run npm test now");
  });

  it("drops an unclosed code block that is still streaming", () => {
    expect(plainPreview("Here is the fix:\n```diff\n- old\n+ new")).toBe("Here is the fix:");
  });

  it("cuts long text at the start, or keeps the tail while streaming", () => {
    const long = "a".repeat(100) + " " + "b".repeat(200);
    expect(plainPreview(long, false, 50)).toBe(`${"a".repeat(50)}…`);
    expect(plainPreview(long, true, 50)).toBe(`…${"b".repeat(50)}`);
  });

  it("keeps short plain text as it is", () => {
    expect(plainPreview("  Fixed the   race.  ")).toBe("Fixed the race.");
  });
});
