import { describe, expect, it } from "vitest";
import type { Agent, ConfigOption } from "@/lib/types";
import { plainConfigOptions } from "./composerModes";

describe("plainConfigOptions for Gemini", () => {
  it("names Gemini's Default and YOLO modes with the shared words and keeps Auto Edit and Plan", () => {
    const mode: ConfigOption = {
      id: "mode",
      name: "Mode",
      category: "mode",
      currentValue: "yolo",
      options: [
        { value: "default", name: "Default", description: "Prompts for approval" },
        { value: "autoEdit", name: "Auto Edit", description: "Auto-approves edit tools" },
        { value: "yolo", name: "YOLO", description: "Auto-approves all tools" },
        { value: "plan", name: "Plan", description: "Read-only mode" },
      ],
    };
    const [o] = plainConfigOptions({ provider: "gemini", configOptions: [mode] } as unknown as Agent);
    expect(o.options.map((c) => c.value)).toEqual(["default", "autoEdit", "yolo", "plan"]);
    expect(o.options.map((c) => c.name)).toEqual(["Ask before changes", "Auto Edit", "Run without asking", "Plan"]);
    expect(o.options[2].description).toBe("Auto-approves all tools (“YOLO”)");
  });
});

describe("plainConfigOptions for Antigravity", () => {
  it("names Antigravity's Auto mode Run without asking", () => {
    const mode: ConfigOption = {
      id: "mode",
      name: "Mode",
      category: "mode",
      currentValue: "auto",
      options: [
        { value: "default", name: "Default", description: null },
        { value: "accept-edits", name: "Accept edits", description: null },
        { value: "auto", name: "Auto", description: null },
        { value: "plan", name: "Plan", description: null },
      ],
    };
    const [o] = plainConfigOptions({ provider: "gemini", configOptions: [mode] } as unknown as Agent);
    expect(o.options.map((c) => c.name)).toEqual(["Ask before changes", "Accept edits", "Run without asking", "Plan"]);
    expect(o.options[2].description).toBe("“Auto”");
  });
});
