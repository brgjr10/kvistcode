import { GitFork } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { CUSTOM_ACCOUNT_ID, PROVIDER_ORDER, PROVIDERS } from "@/lib/providers";
import type { CustomAgent, Provider } from "@/lib/types";
import { errorMessage, modKey } from "@/lib/utils";
import { useApp } from "@/store/app";
import { useUi } from "@/store/ui";
import { ProviderGlyph } from "../ProviderGlyph";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter } from "../ui/dialog";
import { Input, Label, Textarea } from "../ui/input";
import { Kbd } from "../ui/kbd";
import { Spinner } from "../ui/misc";
import { Segmented } from "../ui/segmented";
import { Select } from "../ui/select";

const EMPTY_CUSTOM: CustomAgent[] = [];

/** Fork a parallel agent on its own git worktree rooted at a chosen checkpoint (same or different provider). */
export function ForkDialog() {
  const target = useUi((s) => s.forkTarget);
  const setForkTarget = useUi((s) => s.setForkTarget);
  const openAgent = useUi((s) => s.openAgent);
  const accounts = useApp((s) => s.accounts);
  const customAgents = useApp((s) => s.settings?.customAgents ?? EMPTY_CUSTOM);
  const enabledCustom = useMemo(() => customAgents.filter((c) => c.enabled), [customAgents]);

  const [choice, setChoice] = useState<string>("claude");
  const custom = choice.startsWith("custom:") ? enabledCustom.find((c) => `custom:${c.id}` === choice) : undefined;
  const provider: Provider = custom ? "custom" : (choice as Provider);
  const [accountId, setAccountId] = useState("");
  const [model, setModel] = useState("");
  const [instruction, setInstruction] = useState("");
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);

  const preferredAccount = (p: Provider, fallback?: string) => {
    const list = useApp.getState().accounts.filter((a) => a.provider === p);
    if (fallback && list.some((a) => a.id === fallback)) return fallback;
    return (list.find((a) => a.status?.loggedIn) ?? list[0])?.id ?? "";
  };

  const agent = target?.agent ?? null;
  const checkpointId = target?.checkpointId ?? null;
  const targetKey = target ? `${target.agent.id}:${target.checkpointId ?? "current"}` : null;

  useEffect(() => {
    if (!target) return;
    const src = target.agent;
    const initialChoice = src.provider === "custom" && src.customAgentId ? `custom:${src.customAgentId}` : src.provider;
    setChoice(initialChoice);
    setAccountId(src.provider === "custom" ? CUSTOM_ACCOUNT_ID : preferredAccount(src.provider, src.accountId));
    setModel(src.model ?? src.configChoices?.model ?? "");
    setInstruction("");
    busyRef.current = false;
    setBusy(false);
  }, [targetKey]);

  const chooseAgent = (key: string) => {
    setChoice(key);
    if (key.startsWith("custom:")) {
      setAccountId(CUSTOM_ACCOUNT_ID);
      setModel("");
    } else {
      const p = key as Provider;
      setAccountId(preferredAccount(p, p === agent?.provider ? agent.accountId : undefined));
      if (p !== agent?.provider) setModel("");
    }
  };

  const providerAccounts = useMemo(() => accounts.filter((a) => a.provider === provider), [accounts, provider]);
  const hasLocal = useMemo(() => accounts.some((a) => a.provider === "local"), [accounts]);

  const submit = async () => {
    if (busyRef.current || !agent || (!custom && !accountId)) return;
    busyRef.current = true;
    setBusy(true);
    try {
      const created = await api.forkAgent(agent.id, checkpointId, {
        provider,
        accountId: custom ? CUSTOM_ACCOUNT_ID : accountId,
        customAgentId: custom?.id ?? null,
        model: model.trim() || null,
        instruction: instruction.trim() || null,
      });
      useApp.getState().upsertAgent(created);
      toast.success(`Forked to ${custom ? custom.name : PROVIDERS[provider].short}`, {
        description: created.branch ? `${created.title} · ${created.branch}` : created.title,
      });
      setForkTarget(null);
      openAgent(created.id);
    } catch (e) {
      toast.error("Fork failed", { description: errorMessage(e) });
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  return (
    <Dialog open={!!target} onOpenChange={(o) => !o && !busy && setForkTarget(null)}>
      <DialogContent
        title="Fork from here"
        description={
          agent
            ? `Branch a new isolated worktree from “${agent.title}”${target?.label ? ` (${target.label})` : ""}, seeded with the conversation up to this point.`
            : undefined
        }
      >
        <DialogBody className="space-y-4">
          <div>
            <Label>Provider</Label>
            <Segmented
              aria-label="Provider"
              value={choice}
              onChange={chooseAgent}
              className="w-full max-w-full overflow-x-auto"
              options={[
                ...PROVIDER_ORDER.map((p) => ({
                  value: p as string,
                  label: (
                    <>
                      <ProviderGlyph provider={p} size="xs" />
                      {PROVIDERS[p].short}
                    </>
                  ),
                })),
                ...(hasLocal
                  ? [
                      {
                        value: "local",
                        label: (
                          <>
                            <ProviderGlyph provider="local" size="xs" />
                            {PROVIDERS.local.short}
                          </>
                        ),
                      },
                    ]
                  : []),
                ...enabledCustom.map((c) => ({
                  value: `custom:${c.id}`,
                  label: (
                    <>
                      <ProviderGlyph provider="custom" customAgentId={c.id} size="xs" />
                      {c.name}
                    </>
                  ),
                })),
              ]}
            />
          </div>
          {!custom && (
            <div>
              <Label>Account</Label>
              <Select
                aria-label="Account"
                value={accountId || undefined}
                onValueChange={setAccountId}
                placeholder={providerAccounts.length ? "Choose account" : "No accounts for this provider"}
                disabled={providerAccounts.length === 0}
                options={providerAccounts.map((a) => ({
                  value: a.id,
                  label: `${a.name}${a.status?.loggedIn ? "" : " (not signed in)"}`,
                }))}
              />
            </div>
          )}
          <div>
            <Label htmlFor="fork-model">Model (optional)</Label>
            <Input
              id="fork-model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder="Default model for this provider"
              className="h-8 font-mono text-caption"
            />
          </div>
          <div>
            <Label htmlFor="fork-instruction">Steering instruction (optional)</Label>
            <Textarea
              id="fork-instruction"
              autoFocus
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              placeholder="Leave blank to start the fork ready for your next message, or give it a new direction right away…"
              className="min-h-24"
              onKeyDown={(e) => {
                if (e.key !== "Enter" || !(e.metaKey || e.ctrlKey)) return;
                if (e.nativeEvent.isComposing || e.keyCode === 229 || e.repeat) return;
                e.preventDefault();
                void submit();
              }}
            />
          </div>
        </DialogBody>
        <DialogFooter>
          <span className="mr-auto hidden items-center gap-1 text-meta text-steel sm:flex">
            <Kbd>{modKey}</Kbd>
            <Kbd>⏎</Kbd> to fork
          </span>
          <Button variant="ghost" onClick={() => setForkTarget(null)} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" disabled={busy || (!custom && !accountId)} onClick={() => void submit()}>
            {busy ? <Spinner className="h-3.5 w-3.5 text-current" /> : <GitFork />}
            Fork agent
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
