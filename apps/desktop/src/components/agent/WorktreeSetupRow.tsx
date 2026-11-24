import { Check, CircleX, Play, RotateCcw, ShieldAlert, SkipForward } from "lucide-react";
import { useState } from "react";
import { attempt } from "@/lib/actions";
import { api } from "@/lib/api";
import type { Agent } from "@/lib/types";
import { Button } from "../ui/button";
import { Spinner } from "../ui/misc";

/** Pinned dock section for a worktree agent's setup progress, repo trust gate, or failure output. */
export function WorktreeSetupRow({ agent }: { agent: Agent }) {
  const setup = agent.worktreeSetup;
  const [busy, setBusy] = useState(false);
  const [showOutput, setShowOutput] = useState(false);

  if (!setup) return null;

  const run = async (label: string, fn: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    try {
      await attempt(label, fn);
    } finally {
      setBusy(false);
    }
  };

  if (setup.status === "running") {
    return (
      <div role="status" className="flex items-center gap-3 px-3.5 py-2.5">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-accent/30 bg-accent/10 text-accent">
          <Spinner className="h-4 w-4 text-accent" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-body font-semibold text-fg">Setting up worktree…</div>
          {setup.setupCommand && (
            <div className="truncate font-mono text-caption text-muted">{setup.setupCommand}</div>
          )}
        </div>
      </div>
    );
  }

  if (setup.status === "needs_trust") {
    return (
      <div role="alert" className="space-y-2.5 bg-gradient-to-b from-warn/[0.09] to-transparent px-3.5 py-3">
        <div className="flex items-start gap-3">
          <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-warn/35 bg-warn/10 text-warn">
            <ShieldAlert className="h-4 w-4" aria-hidden />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-body font-semibold text-fg">Approve worktree commands?</div>
            <div className="text-caption leading-snug text-muted">
              <code className="font-mono text-fg">{setup.sourcePath ?? "This repository"}</code> defines shell
              commands to run for new worktrees. Only approve repos you trust.
            </div>
          </div>
        </div>

        <div className="space-y-1.5 pl-11">
          {setup.setupCommand && (
            <div>
              <div className="text-micro font-medium text-steel">Setup</div>
              <pre className="mt-0.5 max-h-24 overflow-auto rounded-lg border border-line bg-base/70 px-2.5 py-1.5 font-mono text-caption text-fg">
                {setup.setupCommand}
              </pre>
            </div>
          )}
          {setup.teardownCommand && (
            <div>
              <div className="text-micro font-medium text-steel">Teardown</div>
              <pre className="mt-0.5 max-h-24 overflow-auto rounded-lg border border-line bg-base/70 px-2.5 py-1.5 font-mono text-caption text-fg">
                {setup.teardownCommand}
              </pre>
            </div>
          )}

          <div className="flex flex-wrap items-center gap-1.5 pt-1">
            <Button
              size="sm"
              variant="warn"
              disabled={busy}
              onClick={() =>
                void run("Couldn't approve worktree hooks", () => api.approveWorktreeHooks(agent.id, true))
              }
            >
              {busy ? <Spinner className="h-3.5 w-3.5 text-current" /> : <Check />}
              Trust &amp; run
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() =>
                void run("Couldn't run worktree hooks", () => api.approveWorktreeHooks(agent.id, false))
              }
            >
              <Play /> Run once
            </Button>
            <Button
              size="sm"
              variant="ghost"
              disabled={busy}
              onClick={() => void run("Couldn't skip worktree setup", () => api.skipWorktreeHooks(agent.id))}
            >
              <SkipForward /> Skip
            </Button>
          </div>
        </div>
      </div>
    );
  }

  // status === "failed"
  return (
    <div role="alert" className="space-y-2 px-3.5 py-2.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-err/35 bg-err/10 text-err">
          <CircleX className="h-4 w-4" aria-hidden />
        </span>
        <div className="min-w-[160px] flex-1">
          <div className="text-body font-semibold text-fg">Worktree setup failed</div>
          <div className="truncate text-caption text-muted">
            {setup.error ?? "Setup command failed"}
            {setup.setupCommand ? ` · ${setup.setupCommand}` : ""}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {setup.output && (
            <Button size="sm" variant="ghost" onClick={() => setShowOutput((v) => !v)}>
              {showOutput ? "Hide output" : "Show output"}
            </Button>
          )}
          <Button
            size="sm"
            variant="secondary"
            disabled={busy}
            onClick={() => void run("Couldn't retry worktree setup", () => api.retryWorktreeSetup(agent.id))}
          >
            {busy ? <Spinner className="h-3.5 w-3.5 text-current" /> : <RotateCcw />}
            Retry
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => void run("Couldn't continue without setup", () => api.skipWorktreeHooks(agent.id))}
          >
            Continue anyway
          </Button>
        </div>
      </div>
      {showOutput && setup.output && (
        <pre className="selectable ml-11 max-h-40 overflow-auto rounded-lg border border-err/25 bg-base/70 px-2.5 py-2 font-mono text-micro leading-relaxed text-muted">
          {setup.output}
        </pre>
      )}
    </div>
  );
}

/** Compact worktree setup status/action block rendered inside Mission Control's AgentTile. */
export function WorktreeSetupCard({ agent, onOpen }: { agent: Agent; onOpen: (id: string) => void }) {
  const setup = agent.worktreeSetup;
  const [busy, setBusy] = useState(false);
  if (!setup) return null;

  const run = async (label: string, fn: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true);
    try {
      await attempt(label, fn);
    } finally {
      setBusy(false);
    }
  };

  if (setup.status === "running") {
    return (
      <div className="relative mx-3 mt-3 flex min-w-0 items-center gap-2 rounded-lg border border-accent/25 bg-accent/[0.06] px-2.5 py-1.5 text-meta text-fg/90">
        <Spinner className="h-3.5 w-3.5 shrink-0 text-accent" />
        <span className="truncate">
          Setting up worktree{setup.setupCommand ? `: ${setup.setupCommand}` : "…"}
        </span>
      </div>
    );
  }

  if (setup.status === "needs_trust") {
    return (
      <div
        className="relative mx-3 mt-3 rounded-lg border border-warn/35 bg-warn/[0.07] p-2"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <div className="flex min-w-0 items-center gap-1.5 text-meta text-warn">
          <ShieldAlert className="h-3.5 w-3.5 shrink-0" />
          <span className="truncate font-medium">
            Approve worktree setup ({setup.sourcePath ?? "repo"})
          </span>
        </div>
        {setup.setupCommand && (
          <div className="mt-1 truncate font-mono text-micro text-muted">{setup.setupCommand}</div>
        )}
        <div className="mt-2 flex gap-1.5">
          <Button
            size="xs"
            variant="warn"
            className="min-w-0 flex-1"
            disabled={busy}
            onClick={() => void run("Couldn't approve worktree hooks", () => api.approveWorktreeHooks(agent.id, true))}
          >
            <Check className="!h-3.5 !w-3.5" />
            <span className="truncate">Trust &amp; run</span>
          </Button>
          <Button
            size="xs"
            variant="ghost"
            className="min-w-0 flex-1"
            disabled={busy}
            onClick={() => void run("Couldn't skip worktree setup", () => api.skipWorktreeHooks(agent.id))}
          >
            <span className="truncate">Skip</span>
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div
      className="relative mx-3 mt-3 rounded-lg border border-err/35 bg-err/[0.07] p-2"
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <div className="flex min-w-0 items-center gap-1.5 text-meta text-err">
        <CircleX className="h-3.5 w-3.5 shrink-0" />
        <span className="truncate font-medium">
          Worktree setup failed{setup.error ? `: ${setup.error}` : ""}
        </span>
      </div>
      <div className="mt-2 flex gap-1.5">
        <Button
          size="xs"
          variant="secondary"
          className="min-w-0 flex-1"
          disabled={busy}
          onClick={() => void run("Couldn't retry worktree setup", () => api.retryWorktreeSetup(agent.id))}
        >
          <RotateCcw className="!h-3.5 !w-3.5" />
          <span className="truncate">Retry</span>
        </Button>
        <Button size="xs" variant="ghost" className="min-w-0 flex-1" onClick={() => onOpen(agent.id)}>
          <span className="truncate">Details</span>
        </Button>
      </div>
    </div>
  );
}
