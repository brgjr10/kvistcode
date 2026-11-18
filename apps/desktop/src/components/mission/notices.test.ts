import { describe, expect, it } from "vitest";
import type { TimelineItem } from "@/lib/types";
import { turnErrorNotice } from "./notices";

let seq = 0;
const base = () => ({ id: `i${++seq}`, agentId: "a", seq, ts: seq });
const notice = (level: "info" | "warn" | "error", text: string) =>
  ({ ...base(), kind: "notice", level, text }) as TimelineItem;
const user = (text: string) => ({ ...base(), kind: "user_message", text }) as TimelineItem;
const said = (text: string) => ({ ...base(), kind: "agent_message", text }) as TimelineItem;

describe("turnErrorNotice", () => {
  it("returns the newest error notice of the current turn", () => {
    const items = [user("go"), notice("error", "Usage limit reached. Resets at 6pm."), notice("info", "Model switched")];
    expect(turnErrorNotice(items)).toBe("Usage limit reached. Resets at 6pm.");
  });

  it("ignores info and warn notices", () => {
    expect(turnErrorNotice([user("go"), notice("info", "Model switched"), said("ok")])).toBeNull();
    expect(turnErrorNotice([user("go"), notice("warn", "Context almost full")])).toBeNull();
  });

  it("does not reach back past the last message sent", () => {
    expect(turnErrorNotice([notice("error", "Old failure"), user("try again"), said("working")])).toBeNull();
  });

  it("handles no timeline", () => {
    expect(turnErrorNotice(undefined)).toBeNull();
    expect(turnErrorNotice([])).toBeNull();
  });
});
