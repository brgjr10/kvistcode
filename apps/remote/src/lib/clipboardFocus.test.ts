import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "./clipboard";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("copyText over plain http", () => {
  it("gives focus back to what had it after the selection-based copy", async () => {
    const button = { focus: vi.fn() };
    const area = { value: "", style: {}, setAttribute: vi.fn(), select: vi.fn(), setSelectionRange: vi.fn(), remove: vi.fn() };
    vi.stubGlobal("window", { isSecureContext: false });
    vi.stubGlobal("document", {
      activeElement: button,
      createElement: () => area,
      body: { appendChild: vi.fn() },
      getSelection: () => null,
      execCommand: () => true,
    });
    expect(await copyText("npm test")).toBe(true);
    expect(area.value).toBe("npm test");
    expect(area.remove).toHaveBeenCalled();
    expect(button.focus).toHaveBeenCalledWith({ preventScroll: true });
  });
});
