import { GitCompare, GitFork } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "@/lib/api";
import { parseUnified } from "@/lib/diff";
import { pluralize } from "@/lib/format";
import type { TurnDiff } from "@/lib/types";
import { errorMessage } from "@/lib/utils";
import { useApp } from "@/store/app";
import { useUi } from "@/store/ui";
import { DiffStat, FileDiffView } from "../DiffView";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter } from "../ui/dialog";
import { Spinner } from "../ui/misc";

/** Lightweight modal showing only the file changes made during one turn (`checkpoint[n-1]..checkpoint[n]`). */
export function TurnDiffDialog() {
  const target = useUi((s) => s.turnDiff);
  const setTurnDiff = useUi((s) => s.setTurnDiff);
  const setForkTarget = useUi((s) => s.setForkTarget);
  const agent = useApp((s) => (target ? s.agents[target.agentId] : undefined));
  const [diff, setDiff] = useState<TurnDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!target) return;
    let live = true;
    setDiff(null);
    setError(null);
    setLoading(true);
    api
      .turnDiff(target.agentId, target.checkpointId)
      .then((res) => {
        if (!live) return;
        setDiff(res);
        setLoading(false);
      })
      .catch((e) => {
        if (!live) return;
        setError(errorMessage(e));
        setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [target]);

  const parsed = useMemo(() => (diff?.patch ? parseUnified(diff.patch) : []), [diff?.patch]);

  const rangeLabel = diff
    ? diff.toSeq !== null
      ? `Checkpoint #${diff.fromSeq} → #${diff.toSeq}`
      : `Checkpoint #${diff.fromSeq} → current files`
    : target?.label;

  return (
    <Dialog open={!!target} onOpenChange={(o) => !o && setTurnDiff(null)}>
      <DialogContent
        className="max-w-4xl"
        title="Turn diff"
        description={
          agent
            ? `${agent.title}${rangeLabel ? ` · ${rangeLabel}` : ""}`
            : (rangeLabel ?? "Changes made during this turn")
        }
      >
        <DialogBody className="max-h-[70vh] space-y-3 overflow-y-auto">
          {loading ? (
            <div className="flex items-center gap-2 py-8 text-small text-steel">
              <Spinner className="h-4 w-4" /> Loading turn diff…
            </div>
          ) : error ? (
            <p role="alert" className="rounded-lg border border-err/30 bg-err/[0.07] px-3 py-2 text-small text-err">
              {error}
            </p>
          ) : !diff || diff.files.length === 0 ? (
            <div className="flex flex-col items-center gap-1.5 rounded-xl border border-dashed border-line-strong py-10 text-center">
              <GitCompare className="h-5 w-5 text-steel" />
              <p className="text-body font-medium text-fg">No file changes in this turn</p>
              <p className="text-caption text-muted">The files at the start and end of this turn are identical.</p>
            </div>
          ) : (
            <>
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-line bg-base/40 px-3 py-2 text-caption">
                <span className="font-medium text-fg">{pluralize(diff.files.length, "file")} changed</span>
                <DiffStat additions={diff.additions} deletions={diff.deletions} />
              </div>
              {diff.truncated && (
                <p
                  role="status"
                  className="rounded-lg border border-warn/25 bg-warn/[0.07] px-3 py-2 text-caption text-muted"
                >
                  This turn's patch was large and truncated at a file boundary. All {diff.files.length} changed files
                  are counted above.
                </p>
              )}
              <div className="space-y-3">
                {parsed.map((f) => (
                  <FileDiffView key={`${f.oldPath ?? ""}:${f.path}`} file={f} defaultOpen={parsed.length <= 6} />
                ))}
              </div>
            </>
          )}
        </DialogBody>
        <DialogFooter>
          {agent && target && (
            <Button
              variant="secondary"
              className="mr-auto"
              onClick={() => {
                setTurnDiff(null);
                setForkTarget({
                  agent,
                  checkpointId: target.checkpointId,
                  label: diff ? `before checkpoint #${diff.fromSeq}` : target.label,
                });
              }}
            >
              <GitFork /> Fork from turn start
            </Button>
          )}
          <Button variant="ghost" onClick={() => setTurnDiff(null)}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
