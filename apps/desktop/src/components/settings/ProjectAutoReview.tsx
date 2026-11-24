import { Check, RotateCcw, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { attempt } from "@/lib/actions";
import { api } from "@/lib/api";
import { CUSTOM_ACCOUNT_ID, PROVIDERS } from "@/lib/providers";
import { normProject } from "@/lib/toolkit";
import type { AutoReviewConfig } from "@/lib/types";
import { useApp } from "@/store/app";
import { Button } from "../ui/button";
import { Input, Label } from "../ui/input";
import { Spinner } from "../ui/misc";
import { Select } from "../ui/select";
import { Switch } from "../ui/switch";

const AUTO_PICK = "auto";

const EMPTY_CONFIG: AutoReviewConfig = {
  enabled: false,
  provider: null,
  accountId: null,
  customAgentId: null,
  model: null,
  autoFixCritical: false,
};

function findProjectConfig(
  map: Record<string, AutoReviewConfig> | undefined,
  project: string,
): AutoReviewConfig {
  if (!map) return EMPTY_CONFIG;
  const want = normProject(project);
  if (map[want]) return map[want];
  for (const [k, v] of Object.entries(map)) {
    if (normProject(k) === want) return v;
  }
  return EMPTY_CONFIG;
}

function sameConfig(a: AutoReviewConfig, b: AutoReviewConfig): boolean {
  const norm = (s: string | null | undefined) => (s ?? "").trim();
  return (
    a.enabled === b.enabled &&
    (a.provider ?? null) === (b.provider ?? null) &&
    norm(a.accountId) === norm(b.accountId) &&
    norm(a.customAgentId) === norm(b.customAgentId) &&
    norm(a.model) === norm(b.model) &&
    a.autoFixCritical === b.autoFixCritical
  );
}

/** Settings → Projects → Auto-review: configure per-project cross-provider review before "Needs you". */
export function ProjectAutoReview({ project }: { project: string }) {
  const settings = useApp((s) => s.settings);
  const accounts = useApp((s) => s.accounts);
  const savedCfg = useMemo(
    () => findProjectConfig(settings?.projectAutoReview, project),
    [settings?.projectAutoReview, project],
  );
  const customAgents = useMemo(
    () => (settings?.customAgents ?? []).filter((c) => c.enabled),
    [settings?.customAgents],
  );

  const [draft, setDraft] = useState<AutoReviewConfig>(savedCfg);
  const [saving, setSaving] = useState(false);

  const reset = useCallback(() => setDraft(savedCfg), [savedCfg]);
  useEffect(() => {
    setDraft(savedCfg);
  }, [savedCfg]);

  const pickerValue = draft.customAgentId
    ? `custom:${draft.customAgentId}`
    : draft.accountId
      ? `acc:${draft.accountId}`
      : draft.provider
        ? `prov:${draft.provider}`
        : AUTO_PICK;

  const options = useMemo(() => {
    const list: { value: string; label: string }[] = [
      { value: AUTO_PICK, label: "Auto (pick another provider)" },
      ...accounts
        .filter((a) => a.provider !== "custom")
        .map((a) => ({ value: `acc:${a.id}`, label: `${PROVIDERS[a.provider].short} · ${a.name}` })),
      ...customAgents.map((c) => ({ value: `custom:${c.id}`, label: c.name })),
    ];
    if (!list.some((o) => o.value === pickerValue)) {
      list.push({ value: pickerValue, label: "Unavailable reviewer" });
    }
    return list;
  }, [accounts, customAgents, pickerValue]);

  const pickReviewer = (v: string) => {
    if (v === AUTO_PICK) {
      setDraft((d) => ({ ...d, provider: null, accountId: null, customAgentId: null }));
      return;
    }
    if (v.startsWith("custom:")) {
      setDraft((d) => ({
        ...d,
        provider: "custom",
        accountId: CUSTOM_ACCOUNT_ID,
        customAgentId: v.slice(7),
      }));
      return;
    }
    if (v.startsWith("acc:")) {
      const acc = accounts.find((a) => a.id === v.slice(4));
      if (acc) {
        setDraft((d) => ({ ...d, provider: acc.provider, accountId: acc.id, customAgentId: null }));
      }
    }
  };

  const dirty = !sameConfig(draft, savedCfg);

  const save = async () => {
    if (!settings || saving) return;
    setSaving(true);
    try {
      const key = normProject(project);
      const nextMap: Record<string, AutoReviewConfig> = {};
      for (const [k, v] of Object.entries(settings.projectAutoReview ?? {})) {
        if (normProject(k) !== key) nextMap[k] = v;
      }
      const cleaned: AutoReviewConfig = {
        enabled: draft.enabled,
        provider: draft.provider,
        accountId: draft.accountId?.trim() || null,
        customAgentId: draft.customAgentId?.trim() || null,
        model: draft.model?.trim() || null,
        autoFixCritical: draft.autoFixCritical,
      };
      const isEmpty =
        !cleaned.enabled &&
        !cleaned.provider &&
        !cleaned.accountId &&
        !cleaned.customAgentId &&
        !cleaned.model &&
        !cleaned.autoFixCritical;
      if (!isEmpty) nextMap[key] = cleaned;
      await attempt("Couldn't save auto-review settings", () =>
        api.updateSettings({ ...settings, projectAutoReview: nextMap }),
      );
      toast.success("Auto-review settings saved");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-caption leading-relaxed text-muted">
        When an agent finishes a turn with code changes, run a read-only second-opinion review with another provider
        before you look at the diff. Findings appear in the Review sheet and on Mission Control cards.
      </p>

      <section className="overflow-hidden rounded-xl border border-line bg-card edge" aria-label="Auto-review settings">
        <header className="flex items-center gap-2 border-b border-line px-4 py-2.5 [&_svg]:h-4 [&_svg]:w-4 [&_svg]:text-steel">
          <ShieldCheck />
          <h3 className="text-body font-semibold text-fg">Cross-provider review for this project</h3>
          <span className="ml-auto text-micro text-steel">Saved in Kvistcode settings</span>
        </header>

        <div className="space-y-4 p-4">
          <div className="flex items-start justify-between gap-4">
            <div>
              <Label htmlFor="ar-enabled" className="mb-0">
                Review changes automatically
              </Label>
              <p className="mt-0.5 text-micro text-steel">
                Runs once per new diff after a turn completes. Individual agents can override this in their Inspector.
              </p>
            </div>
            <Switch
              id="ar-enabled"
              checked={draft.enabled}
              onCheckedChange={(enabled) => setDraft((d) => ({ ...d, enabled }))}
            />
          </div>

          <div className="grid gap-4 border-t border-line pt-4 sm:grid-cols-2">
            <div>
              <Label htmlFor="ar-reviewer">Reviewer provider / account</Label>
              <Select
                id="ar-reviewer"
                aria-label="Reviewer provider and account"
                value={pickerValue}
                onValueChange={pickReviewer}
                options={options}
              />
              <p className="mt-1 text-micro text-steel">
                Auto picks a different installed provider (e.g. Codex for Claude agents, Claude for Codex agents).
              </p>
            </div>

            <div>
              <Label htmlFor="ar-model">Reviewer model (optional)</Label>
              <Input
                id="ar-model"
                className="font-mono text-small"
                placeholder="Account default"
                value={draft.model ?? ""}
                onChange={(e) => setDraft((d) => ({ ...d, model: e.target.value || null }))}
                spellCheck={false}
              />
              <p className="mt-1 text-micro text-steel">
                Leave empty to use the reviewer account&apos;s default model.
              </p>
            </div>
          </div>

          <div className="flex items-start justify-between gap-4 border-t border-line pt-4">
            <div>
              <Label htmlFor="ar-autofix" className="mb-0">
                Send critical findings back once automatically
              </Label>
              <p className="mt-0.5 text-micro text-steel">
                If the reviewer flags critical issues, Kvistcode sends one concise fix prompt back to the agent. Capped
                strictly at 1 auto-fix pass per turn and always respects your budget and turn limits.
              </p>
            </div>
            <Switch
              id="ar-autofix"
              checked={draft.autoFixCritical}
              onCheckedChange={(autoFixCritical) => setDraft((d) => ({ ...d, autoFixCritical }))}
            />
          </div>

          <div className="flex items-center gap-2 pt-1">
            <Button size="sm" disabled={!dirty || saving} onClick={() => void save()}>
              {saving ? <Spinner className="h-3.5 w-3.5 text-current" /> : <Check />}
              Save
            </Button>
            {dirty && (
              <Button size="sm" variant="ghost" disabled={saving} onClick={reset}>
                <RotateCcw /> Reset
              </Button>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}
