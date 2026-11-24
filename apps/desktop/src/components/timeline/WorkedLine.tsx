import { ChevronRight, GitCompare } from "lucide-react";
import { memo, useEffect, useMemo } from "react";
import { pluralize } from "@/lib/format";
import type { TimelineItem } from "@/lib/types";
import { cn } from "@/lib/utils";
import { ensureCheckpoints, useCheckpoints } from "@/store/checkpoints";
import { useUi } from "@/store/ui";
import { formatElapsed } from "./thinking";
import { summarizeFold } from "./turnFold";

/**
 * Minimal look: the one line a finished turn's steps fold into ("Worked for 4m 02s"), with what they
 * were on the right. Opens to show the steps as they are (the timeline renders them as the next rows).
 */
export const WorkedLine = memo(function WorkedLine({
  foldId,
  items,
  durationMs,
  open,
  onToggle,
  agentId,
  turnPromptId,
}: {
  foldId: string;
  /** The fold's steps. */
  items: readonly TimelineItem[];
  /** The turn's duration when it is known; otherwise the line counts the steps. */
  durationMs: number | null;
  open: boolean;
  onToggle: (foldId: string) => void;
  agentId?: string;
  /** The user_message id starting this turn, for looking up its pre-turn checkpoint. */
  turnPromptId?: string | null;
}) {
  useEffect(() => {
    if (agentId) ensureCheckpoints(agentId);
  }, [agentId]);
  const checkpoint = useCheckpoints((s) =>
    agentId && turnPromptId ? s.byAgent[agentId]?.byItem[turnPromptId] : undefined,
  );
  const available = useCheckpoints((s) => (agentId ? (s.byAgent[agentId]?.available ?? false) : false));
  const { text, steps, failed } = useMemo(() => summarizeFold(items), [items]);
  const timed = durationMs !== null && durationMs >= 1000;
  return (
    <div className="tl-worked group/worked flex h-[34px] w-full items-center gap-1.5 border-b border-line text-[13.5px] text-steel">
      <button
        type="button"
        onClick={() => onToggle(foldId)}
        aria-expanded={open}
        className="flex h-full min-w-0 flex-1 items-center gap-1.5 text-left transition-colors hover:text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
      >
        <span className="shrink-0 tabular">
          {timed
            ? `Worked for ${formatElapsed(durationMs)}`
            : steps > 0
              ? `Worked · ${pluralize(steps, "step")}`
              : "Worked"}
        </span>
        <ChevronRight
          aria-hidden
          className={cn("h-3 w-3 shrink-0 transition-transform duration-150", open && "rotate-90")}
        />
        <span className="ml-auto min-w-0 truncate pl-3 text-small" title={text}>
          {text}
        </span>
        {failed > 0 && (
          <span className="shrink-0 text-small text-err/90" aria-label={`${failed} failed`}>
            · {failed} failed
          </span>
        )}
      </button>
      {agentId && available && checkpoint && (
        <button
          type="button"
          onClick={() =>
            useUi.getState().setTurnDiff({
              agentId,
              checkpointId: checkpoint.id,
              label: `checkpoint #${checkpoint.seq}`,
            })
          }
          title="Inspect changes made during this turn"
          aria-label="Inspect turn diff"
          className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-micro text-steel transition-colors hover:bg-raised hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
        >
          <GitCompare className="h-3 w-3" />
          <span>Turn diff</span>
        </button>
      )}
    </div>
  );
});
