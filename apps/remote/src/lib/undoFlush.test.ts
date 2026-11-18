import { afterAll, describe, expect, it, vi } from "vitest";

// The controller listens for the page being hidden to send answers still waiting on their Undo.
const handlers: Record<string, () => void> = {};
const listen = (type: string, fn: () => void) => {
  handlers[type] = fn;
};
const fakeDocument = { addEventListener: listen, removeEventListener: () => {}, visibilityState: "visible" };
vi.stubGlobal("document", fakeDocument);
vi.stubGlobal("window", { addEventListener: listen, removeEventListener: () => {} });
vi.stubGlobal("location", { hash: "", href: "http://phone.test/", search: "" });
vi.stubGlobal("history", { state: null, pushState: () => {}, replaceState: () => {}, back: () => {} });
afterAll(() => {
  vi.unstubAllGlobals();
});

const { respondWithUndo } = await import("./controller");
const { getState } = await import("./store");

describe("swipe answers with Undo", () => {
  it("hiding the app sends the answer and takes its Undo toast away", () => {
    respondWithUndo("a1", "req-1", "allow", "Approved", null);
    expect(getState().toasts.some((t) => t.action?.label === "Undo")).toBe(true);
    expect(getState().resolving["req-1"]).toBe("allow");

    fakeDocument.visibilityState = "hidden";
    handlers.visibilitychange?.();

    // The Undo button would do nothing now: the answer is already on its way.
    expect(getState().toasts.some((t) => t.action?.label === "Undo")).toBe(false);
  });
});
