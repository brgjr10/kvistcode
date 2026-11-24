import {
  AlertCircle,
  AlertTriangle,
  CheckCircle2,
  Lightbulb,
  MessageSquarePlus,
  RefreshCw,
  Send,
  ShieldCheck,
  X,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { pluralize } from "@/lib/format";
import { PROVIDERS } from "@/lib/providers";
import type { Agent, AutoReviewFinding, AutoReviewSeverity, AutoReviewState } from "@/lib/types";
import { cn, errorMessage } from "@/lib/utils";
import { RelTime } from "../RelTime";
import { Button } from "../ui/button";
import { Badge, Spinner } from "../ui/misc";

const SEVERITY_META: Record<
  AutoReviewSeverity,
  { label: string; tone: "err" | "warn" | "neutral"; icon: typeof AlertCircle }
> = {
  critical: { label: "Critical", tone: "err", icon: AlertCircle },
  warning: { label: "Warning", tone: "warn", icon: AlertTriangle },
  suggestion: { label: "Suggestion", tone: "neutral", icon: Lightbulb },
};

export function reviewerLabel(state: AutoReviewState): string {
  const name = PROVIDERS[state.reviewerProvider]?.short ?? state.reviewerProvider;
  return state.reviewerModel ? `${name} · ${state.reviewerModel}` : name;
}

/**
 * Displays cross-provider auto-review status and findings inside `ReviewSheet`,
 * with jump-to-file, apply-as-comment, send-to-agent, dismiss, and re-run actions.
 */
export function AutoReviewPanel({
  agent,
  onJump,
  onApplyComment,
}: {
  agent: Agent;
  onJump: (path: string) => void;
  onApplyComment: (finding: AutoReviewFinding) => void;
}) {
  const state = agent.autoReviewState ?? null;
  const [busyId, setBusyId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  if (!state) return null;

  const isRunning = state.status === "running" || running;
  const findings = state.findings;
  const criticalCount = findings.filter((f) => f.severity === "critical").length;

  const rerun = async () => {
    if (isRunning) return;
    setRunning(true);
    try {
      await api.runAutoReview(agent.id);
    } catch (e) {
      toast.error("Couldn't run review", { description: errorMessage(e) });
    } finally {
      setRunning(false);
    }
  };

  const dismiss = async (findingId: string) => {
    setBusyId(`dismiss:${findingId}`);
    try {
      await api.dismissAutoReviewFinding(agent.id, findingId);
    } catch (e) {
      toast.error("Couldn't dismiss finding", { description: errorMessage(e) });
    } finally {
      setBusyId(null);
    }
  };

  const send = async (findingIds: string[], label: string) => {
    setBusyId(findingIds.length === 1 ? `send:${findingIds[0]}` : "send:all");
    try {
      await api.sendAutoReviewFindings(agent.id, findingIds);
      toast.success(label);
    } catch (e) {
      toast.error("Couldn't send findings", { description: errorMessage(e) });
    } finally {
      setBusyId(null);
    }
  };

  return (
    <section
      aria-label="Cross-provider review"
      className={cn(
        "my-3 overflow-hidden rounded-xl border bg-card edge",
        state.status === "failed"
          ? "border-err/35"
          : criticalCount > 0
            ? "border-err/40"
            : state.status === "findings"
              ? "border-warn/35"
              : state.status === "passed"
                ? "border-ok/30"
                : "border-accent/35",
      )}
    >
      <header className="flex flex-wrap items-center gap-2 border-b border-line px-3.5 py-2.5">
        {isRunning ? (
          <Spinner className="h-3.5 w-3.5 text-accent" />
        ) : state.status === "passed" ? (
          <CheckCircle2 className="h-4 w-4 text-ok" />
        ) : state.status === "failed" ? (
          <AlertCircle className="h-4 w-4 text-err" />
        ) : (
          <ShieldCheck className={cn("h-4 w-4", criticalCount > 0 ? "text-err" : "text-warn")} />
        )}
        <span className="text-small font-semibold text-fg">
          Auto-review · {reviewerLabel(state)}
        </span>
        {state.status === "passed" && <Badge tone="ok">Passed</Badge>}
        {state.status === "running" && <Badge tone="accent">Reviewing…</Badge>}
        {state.status === "failed" && <Badge tone="err">Failed</Badge>}
        {state.status === "findings" && (
          <Badge tone={criticalCount > 0 ? "err" : "warn"}>
            {pluralize(findings.length, "finding")}
            {criticalCount > 0 ? ` · ${criticalCount} critical` : ""}
          </Badge>
        )}
        {state.autoFixSent && <Badge tone="neutral">Auto-fix sent once</Badge>}
        <span className="text-micro text-steel">
          <RelTime ts={state.checkedAt} />
        </span>

        <div className="ml-auto flex items-center gap-1.5">
          {findings.length > 1 && (
            <Button
              size="xs"
              variant="primary"
              disabled={busyId !== null}
              onClick={() => void send([], `Sent ${pluralize(findings.length, "finding")} to the agent`)}
            >
              {busyId === "send:all" ? <Spinner className="h-3 w-3 text-current" /> : <Send className="!h-3 !w-3" />}
              Send all to fix
            </Button>
          )}
          <Button
            size="xs"
            variant="ghost"
            disabled={isRunning}
            onClick={() => void rerun()}
            title="Re-run cross-provider review"
          >
            <RefreshCw className={cn("!h-3 !w-3", isRunning && "animate-spin")} />
            Re-run
          </Button>
        </div>
      </header>

      <div className="px-3.5 py-2.5 text-small">
        <p className={cn("leading-relaxed", state.status === "failed" ? "text-err" : "text-muted")}>
          {state.error || state.summary}
        </p>

        {findings.length > 0 && (
          <ul className="mt-2.5 space-y-2">
            {findings.map((f) => {
              const meta = SEVERITY_META[f.severity];
              const Icon = meta.icon;
              const sending = busyId === `send:${f.id}`;
              const dismissing = busyId === `dismiss:${f.id}`;
              return (
                <li
                  key={f.id}
                  className="rounded-lg border border-line bg-base/60 p-2.5"
                >
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Badge tone={meta.tone} className="gap-1">
                      <Icon className="h-3 w-3" />
                      {meta.label}
                    </Badge>
                    {f.path && (
                      <button
                        type="button"
                        onClick={() => onJump(f.path)}
                        className="rounded px-1 py-0.5 font-mono text-meta text-accent transition-colors hover:bg-accent/10 hover:underline"
                        title={`Jump to ${f.path}`}
                      >
                        {f.path}
                        {f.line ? `:${f.line}` : ""}
                      </button>
                    )}
                    <span className="font-medium text-fg">{f.title}</span>
                    <div className="ml-auto flex items-center gap-1">
                      {f.path && (
                        <Button
                          size="xs"
                          variant="ghost"
                          disabled={busyId !== null}
                          onClick={() => onApplyComment(f)}
                          title="Add as a line comment in this review"
                        >
                          <MessageSquarePlus className="!h-3 !w-3" />
                          Comment
                        </Button>
                      )}
                      <Button
                        size="xs"
                        variant="secondary"
                        disabled={busyId !== null}
                        onClick={() => void send([f.id], "Sent finding to the agent")}
                      >
                        {sending ? <Spinner className="h-3 w-3 text-current" /> : <Send className="!h-3 !w-3" />}
                        Send to fix
                      </Button>
                      <Button
                        size="icon-xs"
                        variant="ghost"
                        aria-label="Dismiss finding"
                        disabled={busyId !== null}
                        onClick={() => void dismiss(f.id)}
                      >
                        {dismissing ? <Spinner className="h-3 w-3" /> : <X className="!h-3.5 !w-3.5" />}
                      </Button>
                    </div>
                  </div>
                  {f.body && f.body !== f.title && (
                    <p className="selectable mt-1.5 whitespace-pre-wrap text-caption leading-relaxed text-muted">
                      {f.body}
                    </p>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </section>
  );
}

/** Inline suggested finding card shown right beneath a matching diff line in ReviewSheet. */
export function InlineFindingCard({
  finding,
  onApply,
  onSend,
  onDismiss,
}: {
  finding: AutoReviewFinding;
  onApply: () => void;
  onSend: () => void;
  onDismiss: () => void;
}) {
  const meta = SEVERITY_META[finding.severity];
  const Icon = meta.icon;
  return (
    <div className="rounded-lg border border-accent/30 bg-card/95 px-3 py-2 text-small shadow-sm">
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge tone={meta.tone} className="gap-1">
          <Icon className="h-3 w-3" />
          {meta.label}
        </Badge>
        <span className="text-micro font-medium uppercase tracking-wide text-steel">Auto-review</span>
        <span className="font-medium text-fg">{finding.title}</span>
        <div className="ml-auto flex items-center gap-1">
          <Button size="xs" variant="ghost" onClick={onApply}>
            <MessageSquarePlus className="!h-3 !w-3" /> Apply as comment
          </Button>
          <Button size="xs" variant="secondary" onClick={onSend}>
            <Send className="!h-3 !w-3" /> Fix
          </Button>
          <Button size="icon-xs" variant="ghost" aria-label="Dismiss finding" onClick={onDismiss}>
            <X className="!h-3.5 !w-3.5" />
          </Button>
        </div>
      </div>
      {finding.body && finding.body !== finding.title && (
        <p className="mt-1 whitespace-pre-wrap text-caption leading-relaxed text-muted">{finding.body}</p>
      )}
    </div>
  );
}

/** Compact summary banner used on Mission Control NeedsYouDetail and AgentTile. */
export function AutoReviewBadge({
  state,
  compact = false,
}: {
  state: AutoReviewState;
  compact?: boolean;
}) {
  const critical = state.findings.filter((f) => f.severity === "critical").length;
  if (state.status === "running") {
    return (
      <span className="inline-flex h-5 items-center gap-1 rounded-full border border-accent/35 bg-accent/10 px-1.5 text-micro font-medium text-accent">
        <Spinner className="h-2.5 w-2.5 text-current" />
        {compact ? "Reviewing" : "Reviewing…"}
      </span>
    );
  }
  if (state.status === "passed") {
    return (
      <span
        title={`${reviewerLabel(state)}: ${state.summary}`}
        className="inline-flex h-5 items-center gap-1 rounded-full border border-ok/30 bg-ok/10 px-1.5 text-micro font-medium text-ok"
      >
        <CheckCircle2 className="h-3 w-3" />
        {compact ? "Reviewed" : "Review passed"}
      </span>
    );
  }
  if (state.status === "failed") {
    return (
      <span
        title={state.error ?? state.summary}
        className="inline-flex h-5 items-center gap-1 rounded-full border border-err/30 bg-err/10 px-1.5 text-micro font-medium text-err"
      >
        <AlertCircle className="h-3 w-3" />
        Review failed
      </span>
    );
  }
  return (
    <span
      title={`${reviewerLabel(state)}: ${state.summary}`}
      className={cn(
        "inline-flex h-5 items-center gap-1 rounded-full border px-1.5 text-micro font-medium",
        critical > 0
          ? "border-err/35 bg-err/10 text-err"
          : "border-warn/35 bg-warn/10 text-warn",
      )}
    >
      <ShieldCheck className="h-3 w-3" />
      {pluralize(state.findings.length, "finding")}
      {!compact && critical > 0 ? ` (${critical} critical)` : ""}
    </span>
  );
}
