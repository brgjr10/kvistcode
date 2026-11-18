import { describe, expect, it } from "vitest";
import { isLook, LOOKS, lookOf } from "./look";

describe("lookOf (the saved look)", () => {
  it("opens a saved Classic or Minimal, or nothing saved, as Clean; keeps Terminal", () => {
    expect(lookOf("classic")).toBe("clean");
    expect(lookOf("minimal")).toBe("clean");
    expect(lookOf(null)).toBe("clean");
    expect(lookOf("clean")).toBe("clean");
    expect(lookOf("terminal")).toBe("terminal");
  });

  it("offers only Clean and Terminal in Settings", () => {
    expect(LOOKS.map((l) => l.id)).toEqual(["clean", "terminal"]);
    expect(isLook("minimal")).toBe(false);
  });
});
