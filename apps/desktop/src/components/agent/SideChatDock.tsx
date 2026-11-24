import { ChevronDown, MessageCircleQuestion, Trash2, X } from "lucide-react";
import { memo } from "react";
import { cn } from "@/lib/utils";
import { useSideChat } from "@/store/sideChat";
import { Markdown } from "../Markdown";
import { Spinner } from "../ui/misc";

/**
 * Collapsible inline card above the composer showing non-interrupting `/btw` side-chat
 * questions and answers while the main agent keeps working.
 */
export const SideChatDock = memo(function SideChatDock({ agentId }: { agentId: string }) {
  const entries = useSideChat((s) => s.byAgent[agentId]);
  const collapsed = useSideChat((s) => s.collapsed[agentId] ?? false);
  const toggleCollapsed = useSideChat((s) => s.toggleCollapsed);
  const dismiss = useSideChat((s) => s.dismiss);
  const clear = useSideChat((s) => s.clear);

  if (!entries || entries.length === 0) return null;
  const running = entries.some((e) => e.status === "running");

  return (
    <section
      aria-label="Side-chat answers"
      className="mb-2 overflow-hidden rounded-xl border border-line-strong bg-card/90 shadow-sm edge"
    >
      <div className="flex h-8 items-center gap-2 border-b border-line/70 px-3 text-caption">
        <button
          type="button"
          onClick={() => toggleCollapsed(agentId)}
          aria-expanded={!collapsed}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left font-medium text-fg transition-colors hover:text-accent"
        >
          <ChevronDown className={cn("h-3.5 w-3.5 shrink-0 text-steel transition-transform", collapsed && "-rotate-90")} />
          {running ? (
            <Spinner className="h-3 w-3 shrink-0 text-accent" />
          ) : (
            <MessageCircleQuestion className="h-3.5 w-3.5 shrink-0 text-accent" />
          )}
          <span className="truncate">Side-chat (/btw)</span>
          <span className="shrink-0 text-micro font-normal text-steel">
            {running ? "answering…" : `${entries.length} · read-only`}
          </span>
        </button>
        <button
          type="button"
          onClick={() => clear(agentId)}
          title="Clear side-chat"
          aria-label="Clear side-chat"
          className="inline-flex h-5 w-5 items-center justify-center rounded text-steel transition-colors hover:bg-raised hover:text-fg"
        >
          <Trash2 className="h-3 w-3" />
        </button>
      </div>
      {!collapsed && (
        <div className="max-h-64 divide-y divide-line/60 overflow-y-auto">
          {entries.map((entry) => (
            <div key={entry.id} className="group/btw px-3 py-2 text-small">
              <div className="flex items-start gap-2">
                <span className="mt-0.5 shrink-0 rounded bg-raised px-1.5 py-0.5 font-mono text-micro text-accent">
                  /btw
                </span>
                <p className="min-w-0 flex-1 font-medium text-fg">{entry.question}</p>
                <button
                  type="button"
                  onClick={() => dismiss(agentId, entry.id)}
                  aria-label="Dismiss side question"
                  className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded text-steel opacity-0 transition-opacity hover:bg-raised hover:text-fg focus-visible:opacity-100 group-hover/btw:opacity-100"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
              <div className="mt-1.5 pl-1 text-caption text-muted">
                {entry.status === "error" ? (
                  <p role="alert" className="text-err">
                    {entry.error ?? "Couldn't answer this side question."}
                  </p>
                ) : entry.answer ? (
                  <div className="selectable prose-sm max-w-none">
                    <Markdown text={entry.answer} />
                  </div>
                ) : (
                  <span className="inline-flex items-center gap-1.5 text-steel">
                    <Spinner className="h-3 w-3" /> Thinking (read-only)…
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
});
