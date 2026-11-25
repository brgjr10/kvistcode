import { describe, expect, it } from "vitest";
import { mockRegistry } from "@/lib/mock-data";
import type { CustomAgent } from "@/lib/types";
import { filterRegistry, isAdded, registryWarning } from "./registryLogic";

const custom = (name: string): CustomAgent => ({
  id: "x",
  name,
  command: "c",
  args: [],
  env: [],
  color: "#fff",
  enabled: true,
});

describe("filterRegistry", () => {
  const list = mockRegistry();

  it("lists agents that can be added before the ones that can't", () => {
    const out = filterRegistry(list, "");
    expect(out).toHaveLength(list.length);
    const firstBlocked = out.findIndex((a) => a.unsupported !== null);
    expect(firstBlocked).toBeGreaterThan(0);
    expect(out.slice(firstBlocked).every((a) => a.unsupported !== null)).toBe(true);
    const names = out.slice(0, firstBlocked).map((a) => a.name);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
  });

  it("searches name, id and description, ignoring case", () => {
    expect(filterRegistry(list, "QWEN").map((a) => a.id)).toEqual(["qwen-code"]);
    expect(filterRegistry(list, "jetbrains").map((a) => a.id)).toEqual(["junie"]);
    expect(filterRegistry(list, "copilot-cli")).toHaveLength(1);
    expect(filterRegistry(list, "no such agent")).toEqual([]);
  });
});

describe("isAdded", () => {
  const qwen = mockRegistry().find((a) => a.id === "qwen-code")!;
  it("matches by name, ignoring case and spaces", () => {
    expect(isAdded(qwen, [custom(" qwen code ")])).toBe(true);
    expect(isAdded(qwen, [custom("Goose")])).toBe(false);
  });
});

describe("registryWarning", () => {
  it("says plainly that it runs on the computer", () => {
    const text = registryWarning({ name: "Qwen Code", source: "npm package @qwen-code/qwen-code@0.28.4" });
    expect(text).toContain("run npm package @qwen-code/qwen-code@0.28.4 on your computer");
    expect(text).toContain("third-party");
  });
});
