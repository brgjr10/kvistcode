import { Check, FileCode2, GitBranch, RotateCcw, ShieldAlert, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { attempt } from "@/lib/actions";
import { api } from "@/lib/api";
import { normProject } from "@/lib/toolkit";
import type { ProjectWorktreeConfig, ResolvedWorktreeHooks } from "@/lib/types";
import { useApp } from "@/store/app";
import { Button } from "../ui/button";
import { Input, Label, Textarea } from "../ui/input";
import { Badge, Spinner } from "../ui/misc";

const SOURCE_LABELS: Record<string, string> = {
  kvistcode_json: ".kvistcode.json",
  cursor_json: ".cursor/worktrees.json",
  conductor_json: "conductor.json",
};

function parseCopyFiles(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || seen.has(line)) continue;
    seen.add(line);
    out.push(line);
  }
  return out;
}

function sameConfig(a: ProjectWorktreeConfig, b: ProjectWorktreeConfig): boolean {
  const normCmd = (s: string | null | undefined) => (s ?? "").trim();
  return (
    normCmd(a.setupCommand) === normCmd(b.setupCommand) &&
    normCmd(a.teardownCommand) === normCmd(b.teardownCommand) &&
    a.copyFiles.length === b.copyFiles.length &&
    a.copyFiles.every((v, i) => v === b.copyFiles[i])
  );
}

/** Settings → Projects → Worktrees: per-project copy globs, setup/teardown commands, and repo config trust. */
export function ProjectWorktrees({ project }: { project: string }) {
  const settings = useApp((s) => s.settings);
  const [resolved, setResolved] = useState<ResolvedWorktreeHooks | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [trustBusy, setTrustBusy] = useState(false);

  const [copyText, setCopyText] = useState("");
  const [setupCmd, setSetupCmd] = useState("");
  const [teardownCmd, setTeardownCmd] = useState("");

  const syncForm = useCallback((cfg: ProjectWorktreeConfig) => {
    setCopyText(cfg.copyFiles.join("\n"));
    setSetupCmd(cfg.setupCommand ?? "");
    setTeardownCmd(cfg.teardownCommand ?? "");
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const next = await api.getWorktreeHooks(project);
      setResolved(next);
      syncForm(next.settings);
    } catch (e) {
      toast.error("Couldn't load worktree hooks", { description: String(e) });
    } finally {
      setLoading(false);
    }
  }, [project, syncForm]);

  useEffect(() => {
    void load();
  }, [load, settings?.projectWorktrees, settings?.trustedWorktreeHooks]);

  const draft: ProjectWorktreeConfig = {
    copyFiles: parseCopyFiles(copyText),
    setupCommand: setupCmd.trim() || null,
    teardownCommand: teardownCmd.trim() || null,
  };
  const savedCfg: ProjectWorktreeConfig = resolved?.settings ?? {
    copyFiles: [],
    setupCommand: null,
    teardownCommand: null,
  };
  const dirty = !sameConfig(draft, savedCfg);

  const save = async () => {
    if (!settings || saving) return;
    setSaving(true);
    try {
      const key = normProject(project);
      const nextMap: Record<string, ProjectWorktreeConfig> = {};
      for (const [k, v] of Object.entries(settings.projectWorktrees ?? {})) {
        if (normProject(k) !== key) nextMap[k] = v;
      }
      const isEmpty = draft.copyFiles.length === 0 && !draft.setupCommand && !draft.teardownCommand;
      if (!isEmpty) nextMap[key] = draft;
      await attempt("Couldn't save worktree hooks", () =>
        api.updateSettings({ ...settings, projectWorktrees: nextMap }),
      );
      const refreshed = await api.getWorktreeHooks(project);
      setResolved(refreshed);
      syncForm(refreshed.settings);
      toast.success("Worktree hooks saved");
    } finally {
      setSaving(false);
    }
  };

  const setTrust = async (hash: string | null) => {
    if (trustBusy) return;
    setTrustBusy(true);
    try {
      const next = await attempt("Couldn't update repo trust", () => api.setWorktreeHooksTrust(project, hash));
      if (next) setResolved(next);
    } finally {
      setTrustBusy(false);
    }
  };

  if (loading && !resolved) {
    return (
      <div className="flex items-center gap-2 py-6 text-small text-muted">
        <Spinner className="h-4 w-4" /> Loading worktree hooks…
      </div>
    );
  }

  const repo = resolved?.repo ?? null;

  return (
    <div className="space-y-4">
      <p className="text-caption leading-relaxed text-muted">
        Run automatically when Kvistcode creates or deletes a git worktree for this project. Environment variables{" "}
        <code className="font-mono text-fg">KVISTCODE_WORKTREE_PATH</code>,{" "}
        <code className="font-mono text-fg">KVISTCODE_PROJECT_PATH</code>,{" "}
        <code className="font-mono text-fg">KVISTCODE_BRANCH</code> and{" "}
        <code className="font-mono text-fg">ROOT_WORKTREE_PATH</code> are set for every command.
      </p>

      <section className="overflow-hidden rounded-xl border border-line bg-card edge" aria-label="Project worktree hooks">
        <header className="flex items-center gap-2 border-b border-line px-4 py-2.5 [&_svg]:h-4 [&_svg]:w-4 [&_svg]:text-steel">
          <GitBranch />
          <h3 className="text-body font-semibold text-fg">Your hooks for this project</h3>
          <span className="ml-auto text-micro text-steel">Saved in Kvistcode settings</span>
        </header>

        <div className="space-y-4 p-4">
          <div>
            <Label htmlFor="wt-copy-files">Files to copy into new worktrees</Label>
            <Textarea
              id="wt-copy-files"
              rows={3}
              className="font-mono text-small"
              placeholder={".env\n.env.local\nconfig/*.local.json"}
              value={copyText}
              onChange={(e) => setCopyText(e.target.value)}
            />
            <p className="mt-1 text-micro text-steel">
              One relative path or glob per line. Copied from the main checkout without overwriting existing files.
            </p>
          </div>

          <div>
            <Label htmlFor="wt-setup-cmd">Setup command (after creation)</Label>
            <Input
              id="wt-setup-cmd"
              className="font-mono text-small"
              placeholder="npm ci"
              value={setupCmd}
              onChange={(e) => setSetupCmd(e.target.value)}
            />
            <p className="mt-1 text-micro text-steel">
              Runs inside the new worktree before the agent starts. Overrides any repo-level setup command.
            </p>
          </div>

          <div>
            <Label htmlFor="wt-teardown-cmd">Teardown command (before removal)</Label>
            <Input
              id="wt-teardown-cmd"
              className="font-mono text-small"
              placeholder="docker compose down"
              value={teardownCmd}
              onChange={(e) => setTeardownCmd(e.target.value)}
            />
            <p className="mt-1 text-micro text-steel">
              Runs inside the worktree right before Kvistcode deletes it. Overrides any repo-level teardown command.
            </p>
          </div>

          <div className="flex items-center gap-2 pt-1">
            <Button size="sm" disabled={!dirty || saving} onClick={() => void save()}>
              {saving ? <Spinner className="h-3.5 w-3.5 text-current" /> : <Check />}
              Save
            </Button>
            {dirty && (
              <Button size="sm" variant="ghost" disabled={saving} onClick={() => syncForm(savedCfg)}>
                <RotateCcw /> Reset
              </Button>
            )}
          </div>
        </div>
      </section>

      <section className="overflow-hidden rounded-xl border border-line bg-card edge" aria-label="Repository config">
        <header className="flex items-center gap-2 border-b border-line px-4 py-2.5 [&_svg]:h-4 [&_svg]:w-4 [&_svg]:text-steel">
          <FileCode2 />
          <h3 className="text-body font-semibold text-fg">Repository config</h3>
          {repo && (
            <span className="ml-auto flex items-center gap-2">
              <code className="font-mono text-micro text-steel">
                {SOURCE_LABELS[repo.source] ?? repo.sourcePath}
              </code>
              {repo.scriptHash && (
                <Badge tone={repo.trusted ? "ok" : "warn"}>
                  {repo.trusted ? "Trusted" : "Needs approval"}
                </Badge>
              )}
            </span>
          )}
        </header>

        {!repo ? (
          <p className="px-4 py-3 text-small leading-relaxed text-steel">
            No repo config found. Add a <code className="font-mono text-muted">.kvistcode.json</code> file at the
            project root (or <code className="font-mono text-muted">.cursor/worktrees.json</code> /{" "}
            <code className="font-mono text-muted">conductor.json</code>). Repo commands always ask once before running.
          </p>
        ) : repo.parseError ? (
          <div className="flex items-start gap-2.5 px-4 py-3 text-small text-warn">
            <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
            <div>
              <div className="font-medium">Ignored {repo.sourcePath}</div>
              <div className="text-caption text-muted">{repo.parseError}</div>
            </div>
          </div>
        ) : (
          <div className="space-y-3 p-4">
            {repo.copyFiles.length > 0 && (
              <div>
                <div className="text-micro font-medium text-muted">Files to copy</div>
                <div className="mt-1 flex flex-wrap gap-1.5">
                  {repo.copyFiles.map((f) => (
                    <Badge key={f} tone="neutral" className="font-mono">
                      {f}
                    </Badge>
                  ))}
                </div>
              </div>
            )}

            {repo.setupCommand && (
              <div>
                <div className="flex items-center gap-2 text-micro font-medium text-muted">
                  <span>Setup command</span>
                  {!resolved?.setupFromRepo && <Badge tone="neutral">Overridden above</Badge>}
                </div>
                <pre className="mt-1 overflow-x-auto rounded-lg border border-line bg-base/60 px-3 py-2 font-mono text-caption text-fg">
                  {repo.setupCommand}
                </pre>
              </div>
            )}

            {repo.teardownCommand && (
              <div>
                <div className="flex items-center gap-2 text-micro font-medium text-muted">
                  <span>Teardown command</span>
                  {!resolved?.teardownFromRepo && <Badge tone="neutral">Overridden above</Badge>}
                </div>
                <pre className="mt-1 overflow-x-auto rounded-lg border border-line bg-base/60 px-3 py-2 font-mono text-caption text-fg">
                  {repo.teardownCommand}
                </pre>
              </div>
            )}

            {repo.scriptHash && (
              <div className="flex flex-wrap items-center justify-between gap-2 border-t border-line pt-3">
                <div className="flex items-center gap-1.5 text-caption text-muted">
                  {repo.trusted ? (
                    <>
                      <ShieldCheck className="h-3.5 w-3.5 text-ok" />
                      <span>Approved for this exact command hash. Changes to the file will ask again.</span>
                    </>
                  ) : (
                    <>
                      <ShieldAlert className="h-3.5 w-3.5 text-warn" />
                      <span>Kvistcode will ask before running these commands on a new worktree.</span>
                    </>
                  )}
                </div>
                {repo.trusted ? (
                  <Button size="sm" variant="ghost" disabled={trustBusy} onClick={() => void setTrust(null)}>
                    Revoke trust
                  </Button>
                ) : (
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={trustBusy}
                    onClick={() => void setTrust(repo.scriptHash)}
                  >
                    Trust repo commands
                  </Button>
                )}
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
