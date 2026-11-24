import { Check, Copy, FolderOpen, GitBranch, RefreshCw, TriangleAlert, Zap } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { PROVIDER_ORDER, PROVIDERS } from "@/lib/providers";
import type { Provider, Trigger, TriggerSource, TriggerWebhookInfo } from "@/lib/types";
import { cn, errorMessage } from "@/lib/utils";
import { pickFolder } from "@/lib/window";
import { useApp } from "@/store/app";
import { ProviderGlyph } from "../ProviderGlyph";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter } from "../ui/dialog";
import { Input, Label, Textarea } from "../ui/input";
import { Spinner } from "../ui/misc";
import { Segmented } from "../ui/segmented";
import { Select } from "../ui/select";
import { Switch } from "../ui/switch";
import { authHelp, defaultAccount, SOURCE_HELP, SOURCE_LABEL, starterPrompt } from "./triggers";

/** Create/edit dialog. `trigger === null` closes it; an object with `id: ""` creates. */
export function TriggerDialog({
  trigger,
  listenerOn,
  onClose,
  onSaved,
}: {
  trigger: Trigger | null;
  listenerOn: boolean;
  onClose: () => void;
  onSaved: (t: Trigger) => void;
}) {
  // Keep rendering the last trigger while the dialog animates out.
  const last = useRef<Trigger | null>(trigger);
  if (trigger) last.current = trigger;
  const shown = last.current;
  return (
    <Dialog open={trigger !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={shown?.id === "" ? "New trigger" : "Edit trigger"}
        description="Starts an agent when something happens elsewhere."
        className="w-[min(640px,calc(100vw-32px))]"
      >
        {shown && (
          <TriggerForm
            key={`${shown.id || "new"}-${shown.updatedAt}`}
            initial={shown}
            listenerOn={listenerOn}
            onClose={onClose}
            onSaved={onSaved}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function TriggerForm({
  initial,
  listenerOn,
  onClose,
  onSaved,
}: {
  initial: Trigger;
  listenerOn: boolean;
  onClose: () => void;
  onSaved: (t: Trigger) => void;
}) {
  const accounts = useApp((s) => s.accounts);
  const [draft, setDraft] = useState<Trigger>(initial);
  const [saving, setSaving] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const t = setTimeout(() => nameRef.current?.focus(), 60);
    return () => clearTimeout(t);
  }, []);

  const set = <K extends keyof Trigger>(key: K, value: Trigger[K]) => setDraft((d) => ({ ...d, [key]: value }));
  const providerAccounts = useMemo(
    () => accounts.filter((a) => a.provider === draft.provider),
    [accounts, draft.provider],
  );

  const chooseSource = (source: TriggerSource) =>
    setDraft((d) => ({
      ...d,
      source,
      // Swap the starting text only if the user hasn't written their own.
      prompt: !d.prompt.trim() || d.prompt === starterPrompt(d.source) ? starterPrompt(source) : d.prompt,
    }));

  const chooseProvider = (p: Provider) =>
    setDraft((d) => ({ ...d, provider: p, customAgentId: null, accountId: defaultAccount(accounts, p)?.id ?? "" }));

  const browse = async () => {
    const p = await pickFolder(draft.projectPath || undefined);
    if (p) set("projectPath", p);
  };

  const save = async () => {
    if (saving) return;
    setSaving(true);
    try {
      const saved = await api.saveTrigger({
        ...draft,
        name: draft.name.trim(),
        prompt: draft.prompt.trim(),
        projectPath: draft.projectPath.trim(),
        keyword: draft.keyword.trim(),
        model: draft.model?.trim() || null,
      });
      onSaved(saved);
      toast.success(initial.id ? "Trigger updated" : "Trigger created", { description: saved.name });
      onClose();
    } catch (e) {
      toast.error("Couldn't save trigger", { description: errorMessage(e) });
    } finally {
      setSaving(false);
    }
  };

  const github = draft.source !== "webhook";

  return (
    <form
      className="flex min-h-0 flex-1 flex-col"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <DialogBody className="space-y-4 pt-2">
        {initial.id && initial.lastError && (
          <div
            role="note"
            className="flex items-start gap-2 rounded-lg border border-warn/30 bg-warn/[0.07] px-3 py-2 text-small"
          >
            <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warn" />
            <span className="min-w-0 text-muted">
              <span className="font-medium text-fg">The last event didn't start an agent.</span>{" "}
              <span className="selectable break-words">{initial.lastError}</span>
            </span>
          </div>
        )}
        <div className="grid grid-cols-[1fr_auto] items-end gap-3">
          <div>
            <Label htmlFor="trg-name">Name</Label>
            <Input
              id="trg-name"
              ref={nameRef}
              value={draft.name}
              onChange={(e) => set("name", e.target.value)}
              placeholder="e.g. Fix failing CI"
            />
          </div>
          <label className="flex h-9 items-center gap-2 text-small text-muted">
            Enabled
            <Switch checked={draft.enabled} onCheckedChange={(v) => set("enabled", v)} aria-label="Enabled" />
          </label>
        </div>

        <div>
          <Label>When</Label>
          <Segmented
            aria-label="Event source"
            value={draft.source}
            onChange={(v) => chooseSource(v as TriggerSource)}
            options={(Object.keys(SOURCE_LABEL) as TriggerSource[]).map((s) => ({
              value: s,
              label: SOURCE_LABEL[s],
            }))}
          />
          <p className="mt-2 text-meta text-steel">{SOURCE_HELP[draft.source]}</p>
        </div>

        {draft.source === "github_mention" && (
          <div>
            <Label htmlFor="trg-keyword">Word to look for</Label>
            <Input
              id="trg-keyword"
              value={draft.keyword}
              onChange={(e) => set("keyword", e.target.value)}
              placeholder="@kvistcode"
              className="font-mono text-small"
            />
          </div>
        )}
        {github && (
          <label className="flex items-center gap-2 text-small text-fg">
            <Switch
              checked={draft.allowOutsiders}
              onCheckedChange={(v) => set("allowOutsiders", v)}
              aria-label="Also react to people without write access"
            />
            Also react to people without write access
          </label>
        )}
        {github && draft.allowOutsiders && (
          <p role="note" className="-mt-2 text-meta text-warn">
            Anyone who can comment on the repository can then start an agent. Keep it asking and keep the daily limit
            low.
          </p>
        )}

        {draft.source === "webhook" && (
          <div>
            <Label>Request check</Label>
            <Segmented
              size="sm"
              aria-label="How requests prove themselves"
              value={draft.auth}
              onChange={(v) => set("auth", v as Trigger["auth"])}
              options={[
                { value: "hmac", label: "Signature (HMAC)" },
                { value: "bearer", label: "Bearer token" },
              ]}
            />
            <p className="mt-2 text-meta text-steel">{authHelp(draft.auth)}</p>
          </div>
        )}
        {draft.source === "webhook" && initial.id && <WebhookDetails trigger={initial} listenerOn={listenerOn} />}
        {draft.source === "webhook" && !initial.id && (
          <p className="text-meta text-steel">The address and secret to give the sender appear after you save.</p>
        )}

        <div>
          <Label>Provider</Label>
          <div className="-mx-1 overflow-x-auto px-1 pb-0.5">
            <Segmented
              aria-label="Agent runtime"
              value={draft.provider}
              onChange={(v) => chooseProvider(v as Provider)}
              options={PROVIDER_ORDER.map((p) => ({
                value: p as string,
                label: (
                  <>
                    <ProviderGlyph provider={p} size="xs" />
                    {PROVIDERS[p].short}
                  </>
                ),
              }))}
            />
          </div>
          <div className="mt-2">
            <Select
              aria-label="Account"
              value={draft.accountId || undefined}
              onValueChange={(v) => set("accountId", v)}
              disabled={providerAccounts.length === 0}
              placeholder={
                providerAccounts.length ? "Choose account" : `No ${PROVIDERS[draft.provider].short} accounts`
              }
              options={providerAccounts.map((a) => ({
                value: a.id,
                label: (
                  <span className="flex items-center gap-2">
                    <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: a.color }} />
                    <span className="truncate">{a.name}</span>
                    <span className={cn("text-micro", a.status?.loggedIn ? "text-steel" : "text-warn")}>
                      {a.status?.loggedIn ? (a.status.label ?? "Signed in") : "Not signed in"}
                    </span>
                  </span>
                ),
              }))}
            />
          </div>
        </div>

        <div className="grid gap-4 sm:grid-cols-[1fr_200px]">
          <div>
            <Label htmlFor="trg-project">Project folder</Label>
            <div className="flex gap-2">
              <div className="relative flex-1">
                <FolderOpen className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-steel" />
                <Input
                  id="trg-project"
                  value={draft.projectPath}
                  onChange={(e) => set("projectPath", e.target.value)}
                  placeholder="/path/to/project"
                  className="pl-9 font-mono text-small"
                />
              </div>
              <Button variant="secondary" className="h-9" onClick={() => void browse()}>
                Browse…
              </Button>
            </div>
          </div>
          <div>
            <Label htmlFor="trg-model">Model (optional)</Label>
            <Input
              id="trg-model"
              value={draft.model ?? ""}
              onChange={(e) => set("model", e.target.value)}
              placeholder="Provider default"
              className="font-mono text-small"
            />
          </div>
        </div>

        <div>
          <Label htmlFor="trg-prompt">Prompt</Label>
          <Textarea
            id="trg-prompt"
            value={draft.prompt}
            onChange={(e) => set("prompt", e.target.value)}
            placeholder="What should the agent do when this fires?"
            className="min-h-[96px]"
          />
          <p className="mt-1.5 text-meta text-steel">
            <code className="font-mono">{"{{event}}"}</code> is replaced by what happened. It is shown to the agent as
            untrusted data, cut short and with secrets removed; it is added at the end if you leave it out.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
          <label className="flex items-center gap-2 text-small text-fg">
            <Switch
              checked={draft.worktree}
              onCheckedChange={(v) => set("worktree", v)}
              aria-label="Isolated git worktree"
            />
            <GitBranch className="h-3.5 w-3.5 text-steel" />
            Isolated worktree
          </label>
          <label className="flex items-center gap-2 text-small text-fg">
            Daily limit
            <Input
              type="number"
              min={1}
              max={200}
              value={draft.dailyCap}
              onChange={(e) => set("dailyCap", Math.max(1, Math.min(200, Math.floor(Number(e.target.value) || 1))))}
              aria-label="Most runs per day"
              className="h-8 w-20 tabular"
            />
            <span className="text-meta text-steel">runs a day</span>
          </label>
        </div>

        <div className="rounded-xl border border-line bg-base/40 px-3 py-2.5">
          <label className="flex items-center gap-2 text-small text-fg">
            <Switch
              checked={draft.runWithoutAsking}
              onCheckedChange={(v) => set("runWithoutAsking", v)}
              aria-label="Run without asking"
            />
            <Zap className={cn("h-3.5 w-3.5", draft.runWithoutAsking ? "text-warn" : "text-steel")} />
            Run without asking
          </label>
          <p role="note" className={cn("mt-1.5 text-meta", draft.runWithoutAsking ? "text-warn" : "text-steel")}>
            {draft.runWithoutAsking
              ? "The agent edits files and runs commands on its own, driven by text from outside. Someone who can send this trigger an event can steer it. Only use it for events you trust."
              : "The agent asks before editing files or running commands, even if Run without asking is your app-wide default."}
          </p>
        </div>
      </DialogBody>
      <DialogFooter>
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" variant="primary" disabled={saving}>
          {saving ? <Spinner className="h-3.5 w-3.5 text-current" /> : <Check />}
          {initial.id ? "Save changes" : "Create trigger"}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** The private address and secret of a saved webhook trigger, loaded on demand. */
function WebhookDetails({ trigger, listenerOn }: { trigger: Trigger; listenerOn: boolean }) {
  const [info, setInfo] = useState<TriggerWebhookInfo | null>(null);
  const [shown, setShown] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    try {
      setInfo(await api.triggerWebhookInfo(trigger.id));
    } catch (e) {
      toast.error("Couldn't read the webhook details", { description: errorMessage(e) });
    }
  };
  const copy = (text: string, what: string) => {
    void navigator.clipboard.writeText(text).then(
      () => toast.success(`${what} copied`),
      () => toast.error("Couldn't copy"),
    );
  };
  const regenerate = async () => {
    setBusy(true);
    try {
      await api.regenerateTriggerWebhook(trigger.id);
      await load();
      toast.success("New address and secret made", { description: "Update the sender; the old ones stopped working." });
    } catch (e) {
      toast.error("Couldn't make a new address", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2 rounded-xl border border-line bg-base/40 px-3 py-2.5">
      {!listenerOn && (
        <p role="note" className="flex items-center gap-1.5 text-meta text-warn">
          <TriangleAlert className="h-3.5 w-3.5 shrink-0" /> The webhook listener is off, so nothing arrives yet. Turn it
          on in Event triggers.
        </p>
      )}
      {info ? (
        <>
          <Field label="Address" value={info.url} onCopy={() => copy(info.url, "Address")} />
          <Field
            label={info.auth === "bearer" ? "Bearer token" : "Signing secret"}
            value={shown ? info.secret : "•".repeat(24)}
            onCopy={() => copy(info.secret, "Secret")}
            action={
              <button
                type="button"
                className="text-meta text-accent hover:underline"
                onClick={() => setShown((s) => !s)}
              >
                {shown ? "Hide" : "Show"}
              </button>
            }
          />
          <p className="text-meta text-steel">
            The listener only accepts connections from this computer. To receive events from the internet, point a tunnel
            you control (Tailscale Funnel, Cloudflare Tunnel) at this address.
          </p>
        </>
      ) : (
        <p className="text-meta text-steel">The address and secret are kept private until you ask for them.</p>
      )}
      <div className="flex gap-2">
        {!info && (
          <Button size="sm" variant="secondary" onClick={() => void load()}>
            Show address and secret
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void regenerate()}>
          <RefreshCw className="!h-3.5 !w-3.5" /> Make new ones
        </Button>
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  onCopy,
  action,
}: {
  label: string;
  value: string;
  onCopy: () => void;
  action?: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-1 flex items-center justify-between text-meta text-muted">
        {label}
        {action}
      </div>
      <div className="flex items-center gap-2">
        <code className="selectable min-w-0 flex-1 truncate rounded-md border border-line bg-card px-2 py-1 font-mono text-caption">
          {value}
        </code>
        <Button size="sm" variant="secondary" onClick={onCopy} aria-label={`Copy ${label.toLowerCase()}`}>
          <Copy className="!h-3.5 !w-3.5" />
        </Button>
      </div>
    </div>
  );
}
