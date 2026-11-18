import type { Schedule } from "@/lib/types";

/**
 * A new, unsaved schedule with the same settings as `s` (the editor creates it when `id` is empty).
 * It starts paused so two copies never fire at the same time by accident, and without the run record.
 */
export function duplicateSchedule(s: Schedule, now = Date.now()): Schedule {
  return {
    ...s,
    id: "",
    name: s.name ? `${s.name} (copy)` : "",
    enabled: false,
    days: [...s.days],
    lastRunAt: null,
    lastAgentId: null,
    lastError: null,
    updatedAt: now,
  };
}
