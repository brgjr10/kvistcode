import { describe, expect, it } from "vitest";
import { keepsOwnEnter } from "./keys";

/** A stand-in element whose ancestors (itself first) match the given selectors. */
function el(...matches: string[]) {
  return {
    closest: (selector: string) => {
      const wanted = selector.split(",").map((s) => s.trim());
      return matches.some((m) => wanted.includes(m)) ? {} : null;
    },
  } as unknown as EventTarget;
}

describe("keepsOwnEnter", () => {
  it("leaves Enter to buttons, links and other controls", () => {
    expect(keepsOwnEnter(el("button"))).toBe(true);
    expect(keepsOwnEnter(el("a[href]"))).toBe(true);
    expect(keepsOwnEnter(el("[role=button]"))).toBe(true);
    expect(keepsOwnEnter(el("[role=tab]"))).toBe(true);
    expect(keepsOwnEnter(el("summary"))).toBe(true);
  });

  it("takes Enter anywhere else", () => {
    expect(keepsOwnEnter(el())).toBe(false);
    expect(keepsOwnEnter(el("div"))).toBe(false);
    expect(keepsOwnEnter(null)).toBe(false);
    // window / document as the target (nothing focused) have no closest()
    expect(keepsOwnEnter({} as EventTarget)).toBe(false);
  });
});
