import type { Account, Provider, Trigger, TriggerSource } from "@/lib/types";

export const SOURCE_LABEL: Record<TriggerSource, string> = {
  github_ci: "GitHub: CI fails",
  github_mention: "GitHub: comment mention",
  webhook: "Webhook",
};

export const SOURCE_HELP: Record<TriggerSource, string> = {
  github_ci:
    "Starts an agent when a check fails on an open pull request in this project's GitHub repository. Needs the GitHub sign-in (gh). Checked every couple of minutes.",
  github_mention:
    "Starts an agent when a pull request or issue comment mentions the word below. Only comments from the repository's owners, members and collaborators count unless you allow others.",
  webhook:
    "Starts an agent when something sends a request to this trigger's private address (Linear, Sentry, Slack, a script). Needs the webhook listener below to be on.",
};

export function defaultAccount(accounts: Account[], provider: Provider): Account | undefined {
  const mine = accounts.filter((a) => a.provider === provider);
  return mine.find((a) => a.status?.loggedIn) ?? mine[0];
}

/** Blank trigger: isolated and asking, whatever the interactive defaults are. */
export function blankTrigger(accounts: Account[], recentProject: string | undefined): Trigger {
  return {
    id: "",
    name: "",
    enabled: true,
    source: "github_ci",
    provider: "claude",
    accountId: defaultAccount(accounts, "claude")?.id ?? "",
    projectPath: recentProject ?? "",
    prompt: "",
    worktree: true,
    model: null,
    customAgentId: null,
    runWithoutAsking: false,
    keyword: "@kvistcode",
    allowOutsiders: false,
    auth: "hmac",
    dailyCap: 10,
    pathToken: "",
    createdAt: 0,
    updatedAt: 0,
    lastFiredAt: null,
    lastAgentId: null,
    lastError: null,
    lastPolledAt: null,
    firedDay: "",
    firedToday: 0,
    firedTotal: 0,
  };
}

/** A starting prompt for a source, with the event placed where the agent will read it. */
export function starterPrompt(source: TriggerSource): string {
  switch (source) {
    case "github_ci":
      return "CI is failing. Find the cause and fix it.\n\n{{event}}";
    case "github_mention":
      return "Someone asked for help in a comment. Do what they ask, within this project.\n\n{{event}}";
    case "webhook":
      return "Look into this event and propose a fix.\n\n{{event}}";
  }
}

/** Runs counted for today (the count is only valid for the day it was written). */
export function firedToday(t: Pick<Trigger, "firedDay" | "firedToday">, now = new Date()): number {
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  return t.firedDay === today ? t.firedToday : 0;
}

/** One-line description of how a webhook sender proves itself. */
export function authHelp(auth: Trigger["auth"]): string {
  return auth === "hmac"
    ? "Sign the raw request body with HMAC-SHA256 using the secret and send it as X-Hub-Signature-256: sha256=<hex> (X-Kvistcode-Signature, Linear-Signature and Sentry-Hook-Signature also work)."
    : "Send the secret as Authorization: Bearer <secret>.";
}
