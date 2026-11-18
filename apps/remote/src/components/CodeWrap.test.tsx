import { describe, expect, it, vi } from "vitest";
import { renderToString } from "@/test/render";
import { codeHasLongLines } from "@/lib/collapse";

// Hooks and memo are stubbed so components can be called directly (no DOM in these tests).
vi.mock("react", async (importOriginal) => {
  const m = await importOriginal<typeof import("react")>();
  return {
    ...m,
    memo: (c: unknown) => c,
    useMemo: (f: () => unknown) => f(),
    useState: (v: unknown) => [typeof v === "function" ? (v as () => unknown)() : v, () => {}],
    useRef: (v: unknown) => ({ current: v }),
    useEffect: () => {},
    useContext: () => null,
  };
});
vi.mock("./CopyButton", () => ({ CopyButton: () => null }));
vi.mock("./Icon", () => ({ Icon: () => null }));
vi.mock("./LocalImage", () => ({ LocalImage: () => null }));

const { Markdown } = await import("./Markdown");
const html = (text: string) => renderToString(<Markdown text={text} />);

describe("codeHasLongLines", () => {
  it("is false for code that fits across a phone", () => {
    expect(codeHasLongLines("const a = 1;\nconst b = 2;\n")).toBe(false);
    expect(codeHasLongLines("x".repeat(44))).toBe(false);
    expect(codeHasLongLines("")).toBe(false);
  });

  it("is true once any one line runs past the edge", () => {
    expect(codeHasLongLines(`short\n${"y".repeat(45)}\nshort`)).toBe(true);
  });

  it("counts a tab as two columns and starts over on each line", () => {
    expect(codeHasLongLines("\t".repeat(22) + "z")).toBe(true);
    expect(codeHasLongLines(`${"a".repeat(40)}\n${"b".repeat(40)}`)).toBe(false);
  });
});

describe("code block Wrap toggle", () => {
  it("is offered for a block with a long line, starting off (code keeps its shape)", () => {
    const out = html("```sh\nnpx vitest run src/components/timeline/ToolRow.test.tsx --reporter verbose\n```");
    expect(out).toContain('aria-label="Wrap long lines"');
    expect(out).not.toContain("aria-pressed=\"true\""); // false is left out by this renderer
    expect(out).not.toContain("whitespace-pre-wrap");
  });

  it("is left out when every line already fits", () => {
    const out = html("```ts\nconst a = 1;\n```");
    expect(out).not.toContain("Wrap long lines");
  });
});
