import { Pencil, Play, Plus, TriangleAlert, Trash2, Webhook, Zap } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { relativeTimeLong } from "@/lib/format";
import type { Trigger, TriggerWebhookStatus } from "@/lib/types";
import { basename, cn, errorMessage } from "@/lib/utils";
import { useApp } from "@/store/app";
import { useUi } from "@/store/ui";
import { ConfirmDialog } from "../ui/alert-dialog";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Spinner } from "../ui/misc";
import { Switch } from "../ui/switch";
import { TriggerDialog } from "./TriggerDialog";
import { blankTrigger, firedToday, SOURCE_LABEL } from "./triggers";

/** Event triggers: the Schedules page's counterpart for things that happen elsewhere. */
export function TriggersPanel() {
  const [triggers, setTriggers] = useState<Trigger[] | null>(null);
  const [webhook, setWebhook] = useState<TriggerWebhookStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Trigger | null>(null);
  const [deleting, setDeleting] = useState<Trigger | null>(null);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [port, setPort] = useState("");
  const openAgent = useUi((s) => s.openAgent);

  const load = useCallback(async () => {
    try {
      const v = await api.listTriggers();
      setTriggers(v.triggers);
      setWebhook(v.webhook);
      setPort((p) => p || String(v.webhook.port));
      setLoadError(null);
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, []);

  // Triggers fire in the background, so refresh when the window comes back into focus.
  useEffect(() => {
    void load();
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [load]);

  const upsert = (t: Trigger) =>
    setTriggers((list) => {
      if (!list) return [t];
      return list.some((x) => x.id === t.id) ? list.map((x) => (x.id === t.id ? t : x)) : [...list, t];
    });
  const mark = (id: string, on: boolean) => setBusy((b) => ({ ...b, [id]: on }));

  const toggle = async (t: Trigger, enabled: boolean) => {
    mark(t.id, true);
    try {
      upsert(await api.saveTrigger({ ...t, enabled }));
    } catch (e) {
      toast.error(`Couldn't ${enabled ? "enable" : "pause"} “${t.name}”`, { description: errorMessage(e) });
    } finally {
      mark(t.id, false);
    }
  };

  const test = async (t: Trigger) => {
    mark(t.id, true);
    try {
      const agent = await api.testTrigger(t.id);
      useApp.getState().upsertAgent(agent);
      toast.success(`Started “${t.name}” with a test event`, {
        description: agent.title,
        action: { label: "Open", onClick: () => openAgent(agent.id) },
      });
      void load();
    } catch (e) {
      toast.error(`Couldn't test “${t.name}”`, { description: errorMessage(e) });
    } finally {
      mark(t.id, false);
    }
  };

  const confirmDelete = async () => {
    if (!deleting) return;
    const target = deleting;
    mark(target.id, true);
    try {
      await api.deleteTrigger(target.id);
      setTriggers((list) => list?.filter((x) => x.id !== target.id) ?? null);
      toast(`Deleted “${target.name}”`);
      setDeleting(null);
    } catch (e) {
      toast.error("Couldn't delete trigger", { description: errorMessage(e) });
    } finally {
      mark(target.id, false);
    }
  };

  const applyWebhook = async (enabled: boolean) => {
    const n = Number(port);
    try {
      const s = await api.setTriggerWebhook(enabled, Number.isInteger(n) && n > 0 ? n : null);
      setWebhook(s);
      if (s.error) toast.error("The webhook listener didn't start", { description: s.error });
    } catch (e) {
      toast.error("Couldn't change the webhook listener", { description: errorMessage(e) });
    }
  };

  const newTrigger = () => {
    const { accounts, settings } = useApp.getState();
    setEditing(blankTrigger(accounts, settings?.recentProjects[0]));
  };

  return (
    <section aria-label="Event triggers" className="mt-10">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div>
          <h2 className="text-[14px] font-medium text-fg">Event triggers</h2>
          <p className="text-caption text-muted">
            Start an agent when a GitHub check fails, someone mentions a keyword, or a webhook arrives.
          </p>
        </div>
        <Button variant="secondary" onClick={newTrigger}>
          <Plus /> New trigger
        </Button>
      </div>

      {webhook && (
        <div className="mb-3 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-line bg-card px-4 py-3 text-small">
          <label className="flex items-center gap-2 text-fg">
            <Switch
              checked={webhook.enabled}
              onCheckedChange={(v) => void applyWebhook(v)}
              aria-label="Webhook listener"
            />
            <Webhook className="h-3.5 w-3.5 text-steel" />
            Webhook listener
          </label>
          <span className="flex items-center gap-1.5 text-muted">
            Port
            <Input
              value={port}
              onChange={(e) => setPort(e.target.value.replace(/\D/g, "").slice(0, 5))}
              onBlur={() => webhook.enabled && String(webhook.port) !== port && void applyWebhook(true)}
              aria-label="Webhook listener port"
              className="h-8 w-20 tabular"
              inputMode="numeric"
            />
          </span>
          <span className={cn("text-meta", webhook.error ? "text-err" : "text-steel")}>
            {webhook.error ??
              (webhook.running
                ? "Listening on this computer only (127.0.0.1)."
                : "Off. Nothing listens for webhooks until you turn this on.")}
          </span>
        </div>
      )}

      {loadError ? (
        <p role="alert" className="text-small text-err">
          Couldn't load triggers: <span className="selectable font-mono text-caption">{loadError}</span>
        </p>
      ) : triggers === null ? (
        <div className="flex justify-center py-6">
          <Spinner className="h-4 w-4 text-steel" />
        </div>
      ) : triggers.length === 0 ? (
        <p className="rounded-xl border border-dashed border-line-strong px-4 py-6 text-center text-small text-muted">
          No triggers yet. A trigger can fix failing CI, answer a mention, or look into an alert as soon as it arrives.
        </p>
      ) : (
        <ul className="space-y-2.5" aria-label="Event triggers">
          {triggers.map((t) => (
            <li
              key={t.id}
              className={cn(
                "rounded-xl border bg-card px-4 py-3",
                t.enabled ? "border-line hover:border-line-strong" : "border-line/70",
              )}
            >
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                <div className={cn("min-w-0 flex-1", !t.enabled && "opacity-60")}>
                  <div className="flex min-w-0 items-center gap-2">
                    <button
                      type="button"
                      onClick={() => setEditing(t)}
                      className="min-w-0 truncate text-left text-[13.5px] font-medium text-fg hover:underline hover:decoration-line-strong hover:underline-offset-4"
                    >
                      {t.name}
                    </button>
                    {t.runWithoutAsking && (
                      <span className="inline-flex shrink-0 items-center gap-1 rounded-full border border-warn/30 px-2 text-micro text-warn">
                        <Zap className="h-3 w-3" /> No asking
                      </span>
                    )}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-caption text-muted">
                    <span>{SOURCE_LABEL[t.source]}</span>
                    <span className="truncate">{basename(t.projectPath)}</span>
                    <span>
                      {t.lastFiredAt ? `Last fired ${relativeTimeLong(t.lastFiredAt)}` : "Hasn't fired yet"}
                    </span>
                    <span className="tabular">
                      {firedToday(t)} of {t.dailyCap} today
                    </span>
                    {t.lastAgentId && (
                      <button
                        type="button"
                        className="text-accent hover:underline"
                        onClick={() => openAgent(t.lastAgentId as string)}
                      >
                        Open last agent
                      </button>
                    )}
                  </div>
                  {t.lastError && (
                    <p className="mt-1.5 flex items-start gap-1.5 text-meta text-warn">
                      <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                      <span className="selectable min-w-0 break-words">{t.lastError}</span>
                    </p>
                  )}
                </div>
                <div className="flex items-center gap-1.5">
                  <Switch
                    checked={t.enabled}
                    disabled={busy[t.id]}
                    onCheckedChange={(v) => void toggle(t, v)}
                    aria-label={`${t.enabled ? "Pause" : "Enable"} ${t.name}`}
                  />
                  <Button size="sm" variant="secondary" disabled={busy[t.id]} onClick={() => void test(t)}>
                    {busy[t.id] ? <Spinner className="h-3.5 w-3.5 text-current" /> : <Play className="!h-3.5 !w-3.5" />}
                    Test
                  </Button>
                  <Button size="sm" variant="ghost" aria-label={`Edit ${t.name}`} onClick={() => setEditing(t)}>
                    <Pencil className="!h-3.5 !w-3.5" />
                  </Button>
                  <Button size="sm" variant="ghost" aria-label={`Delete ${t.name}`} onClick={() => setDeleting(t)}>
                    <Trash2 className="!h-3.5 !w-3.5" />
                  </Button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}

      <TriggerDialog
        trigger={editing}
        listenerOn={webhook?.running ?? false}
        onClose={() => setEditing(null)}
        onSaved={upsert}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        danger
        title="Delete trigger?"
        description={deleting ? <>“{deleting.name}” will stop firing. Agents it already started are kept.</> : null}
        confirmLabel="Delete trigger"
        onConfirm={confirmDelete}
      />
    </section>
  );
}
