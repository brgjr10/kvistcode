// Browser stand-in for the trigger commands (the real ones are in src-tauri/src/triggers.rs).

import type { KvistcodeApi } from "./api";
import type { Agent, Trigger, TriggersView, TriggerWebhookInfo, TriggerWebhookStatus } from "./types";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const token = () => Array.from({ length: 4 }, () => Math.random().toString(16).slice(2, 10)).join("");

export function createTriggerMock(getApi: () => KvistcodeApi) {
  let triggers: Trigger[] = [];
  const secrets = new Map<string, string>();
  const webhook: TriggerWebhookStatus = { enabled: false, port: 47831, running: false, error: null };
  const view = (): TriggersView => ({
    triggers: structuredClone(triggers).sort((a, b) => a.name.localeCompare(b.name)),
    webhook: { ...webhook },
  });

  return {
    async list(): Promise<TriggersView> {
      await sleep(150);
      return view();
    },

    async save(input: Trigger): Promise<Trigger> {
      await sleep(200);
      const name = input.name.trim();
      if (!name) throw new Error("Give the trigger a name");
      if (!input.prompt.trim()) throw new Error("Write what the agent should do when this fires");
      if (!input.projectPath.trim()) throw new Error("Pick a project folder.");
      if (input.source === "github_mention" && !input.keyword.trim())
        throw new Error("Enter the word to look for, such as @kvistcode");
      if (input.dailyCap < 1 || input.dailyCap > 200) throw new Error("The daily limit must be between 1 and 200");
      const prev = triggers.find((t) => t.id === input.id);
      const now = Date.now();
      const saved: Trigger = {
        ...input,
        name,
        prompt: input.prompt.trim(),
        id: prev?.id ?? `trg-${now.toString(36)}`,
        pathToken: prev?.pathToken ?? token(),
        createdAt: prev?.createdAt ?? now,
        updatedAt: now,
        lastFiredAt: prev?.lastFiredAt ?? null,
        lastAgentId: prev?.lastAgentId ?? null,
        lastError: null,
        lastPolledAt: prev?.lastPolledAt ?? null,
        firedDay: prev?.firedDay ?? "",
        firedToday: prev?.firedToday ?? 0,
        firedTotal: prev?.firedTotal ?? 0,
      };
      if (saved.source === "webhook" && !secrets.has(saved.id)) secrets.set(saved.id, token());
      triggers = prev ? triggers.map((t) => (t.id === saved.id ? saved : t)) : [...triggers, saved];
      return structuredClone(saved);
    },

    async remove(triggerId: string): Promise<void> {
      triggers = triggers.filter((t) => t.id !== triggerId);
      secrets.delete(triggerId);
    },

    async test(triggerId: string): Promise<Agent> {
      const t = triggers.find((x) => x.id === triggerId);
      if (!t) throw new Error("Trigger not found");
      const agent = await getApi().createAgent({
        provider: t.provider,
        accountId: t.accountId,
        projectPath: t.projectPath,
        prompt: `${t.prompt}\n\n(test event)`,
        title: `${t.name} · test`,
        worktree: t.worktree,
        model: t.model,
        groupId: null,
        customAgentId: t.customAgentId,
        attachments: [],
        permissionMode: t.runWithoutAsking ? "auto" : "ask",
      });
      t.lastFiredAt = Date.now();
      t.lastAgentId = agent.id;
      t.firedTotal += 1;
      return agent;
    },

    async setWebhook(enabled: boolean, port: number | null): Promise<TriggerWebhookStatus> {
      if (port !== null && (port < 1024 || port > 65535)) throw new Error("Choose a port between 1024 and 65535");
      webhook.enabled = enabled;
      if (port !== null) webhook.port = port;
      webhook.running = enabled;
      return { ...webhook };
    },

    async webhookInfo(triggerId: string): Promise<TriggerWebhookInfo> {
      const t = triggers.find((x) => x.id === triggerId && x.source === "webhook");
      if (!t) throw new Error("Trigger not found");
      return {
        url: `http://127.0.0.1:${webhook.port}/hook/${t.pathToken}`,
        secret: secrets.get(t.id) ?? "",
        auth: t.auth,
      };
    },

    async regenerate(triggerId: string): Promise<Trigger> {
      const t = triggers.find((x) => x.id === triggerId && x.source === "webhook");
      if (!t) throw new Error("Only webhook triggers have a URL");
      t.pathToken = token();
      secrets.set(t.id, token());
      return structuredClone(t);
    },
  };
}
