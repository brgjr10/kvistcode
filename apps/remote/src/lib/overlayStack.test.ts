import { describe, expect, it } from "vitest";
import { dropOverlay, isTopOverlay, pushOverlay } from "./overlay";

describe("overlay stack", () => {
  it("only the newest overlay is on top (a confirm over a sheet takes Escape)", () => {
    pushOverlay(101);
    expect(isTopOverlay(101)).toBe(true);
    pushOverlay(102);
    expect(isTopOverlay(102)).toBe(true);
    expect(isTopOverlay(101)).toBe(false);
    dropOverlay(102);
    expect(isTopOverlay(101)).toBe(true);
    dropOverlay(101);
    expect(isTopOverlay(101)).toBe(false);
  });

  it("closing a lower overlay first leaves the top one in charge", () => {
    pushOverlay(201);
    pushOverlay(202);
    dropOverlay(201);
    expect(isTopOverlay(202)).toBe(true);
    dropOverlay(202);
    dropOverlay(202); // twice is harmless
    expect(isTopOverlay(202)).toBe(false);
  });
});
