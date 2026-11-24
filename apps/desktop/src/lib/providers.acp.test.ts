import { describe, expect, it } from "vitest";
import { PROVIDERS, PROVIDER_ORDER, providerRank } from "./providers";

describe("OpenCode and Cursor are built-in providers", () => {
  it("have identity metadata and sort before custom agents", () => {
    for (const id of ["opencode", "cursor"] as const) {
      expect(PROVIDERS[id].id).toBe(id);
      expect(PROVIDER_ORDER).toContain(id);
      expect(providerRank(id)).toBeLessThan(providerRank("custom"));
    }
    expect(PROVIDER_ORDER.indexOf("gemini")).toBeLessThan(PROVIDER_ORDER.indexOf("opencode"));
  });
});
