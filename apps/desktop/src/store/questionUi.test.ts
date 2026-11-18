import { describe, expect, it } from "vitest";
import { useQuestionUi } from "./questionUi";

describe("questionUi: shown in the dock", () => {
  it("tracks a question the dock shows apart from whether it owns the number keys, and forgets it on clear", () => {
    const s = useQuestionUi.getState();
    s.setInDock("q-1", true);
    expect(useQuestionUi.getState().inDock).toEqual({ "q-1": true });
    expect(useQuestionUi.getState().docked["q-1"]).toBeUndefined();
    const before = useQuestionUi.getState();
    s.setInDock("q-1", true);
    expect(useQuestionUi.getState()).toBe(before);
    s.clear("q-1");
    expect(useQuestionUi.getState().inDock).toEqual({});
    s.setInDock("q-2", true);
    s.setInDock("q-2", false);
    expect(useQuestionUi.getState().inDock).toEqual({});
  });
});
