import { Check, Container, Info, RefreshCw, RotateCcw, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { attempt } from "@/lib/actions";
import { api } from "@/lib/api";
import { normProject, restartNote, runningAgents } from "@/lib/toolkit";
import type { ProjectSandboxConfig, SandboxCapabilities, SandboxMode } from "@/lib/types";
import { useApp } from "@/store/app";
import { Button } from "../ui/button";
import { Input, Label, Textarea } from "../ui/input";
import { Badge, Spinner } from "../ui/misc";
import { Select } from "../ui/select";
import { Switch } from "../ui/switch";
import { SettingRow } from "./SettingRow";

const DEFAULT_DOCKER_IMAGE = "node:22-bookworm";

const EMPTY_CONFIG: ProjectSandboxConfig = {
  mode: "off",
  allowNetwork: false,
  dockerImage: null,
  extraReadPaths: [],
};

function parseExtraPaths(text: string): string[] {
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

function sameConfig(a: ProjectSandboxConfig, b: ProjectSandboxConfig): boolean {
  const normImg = (s: string | null | undefined) => (s ?? "").trim();
  return (
    a.mode === b.mode &&
    a.allowNetwork === b.allowNetwork &&
    normImg(a.dockerImage) === normImg(b.dockerImage) &&
    a.extraReadPaths.length === b.extraReadPaths.length &&
    a.extraReadPaths.every((v, i) => v === b.extraReadPaths[i])
  );
}

function configForProject(
  map: Record<string, ProjectSandboxConfig> | undefined,
  project: string,
): ProjectSandboxConfig {
  if (!map) return EMPTY_CONFIG;
  if (map[project]) return map[project];
  const want = normProject(project);
  for (const [k, v] of Object.entries(map)) {
    if (normProject(k) === want) return v;
  }
  return EMPTY_CONFIG;
}

/** Settings → Projects → Sandbox: per-project Bubblewrap / Docker isolation for unattended runs. */
export function ProjectSandbox({ project }: { project: string }) {
  const settings = useApp((s) => s.settings);
  const agents = useApp((s) => s.agents);
  const [caps, setCaps] = useState<SandboxCapabilities | null>(null);
  const [probing, setProbing] = useState(true);
  const [saving, setSaving] = useState(false);

  const savedCfg = configForProject(settings?.projectSandboxes, project);
  const [mode, setMode] = useState<SandboxMode>(savedCfg.mode);
  const [allowNetwork, setAllowNetwork] = useState<boolean>(savedCfg.allowNetwork);
  const [dockerImage, setDockerImage] = useState<string>(savedCfg.dockerImage ?? "");
  const [extraPathsText, setExtraPathsText] = useState<string>(savedCfg.extraReadPaths.join("\n"));

  const syncForm = useCallback((cfg: ProjectSandboxConfig) => {
    setMode(cfg.mode);
    setAllowNetwork(cfg.allowNetwork);
    setDockerImage(cfg.dockerImage ?? "");
    setExtraPathsText(cfg.extraReadPaths.join("\n"));
  }, []);

  useEffect(() => {
    syncForm(savedCfg);
  }, [project, savedCfg, syncForm]);

  const probe = useCallback(async () => {
    setProbing(true);
    try {
      const next = await api.detectSandboxBackends();
      setCaps(next);
    } catch (e) {
      toast.error("Couldn't detect sandbox backends", { description: String(e) });
    } finally {
      setProbing(false);
    }
  }, []);

  useEffect(() => {
    void probe();
  }, [probe]);

  const draft: ProjectSandboxConfig = {
    mode,
    allowNetwork,
    dockerImage: dockerImage.trim() || null,
    extraReadPaths: parseExtraPaths(extraPathsText),
  };
  const dirty = !sameConfig(draft, savedCfg);
  const isDefault =
    draft.mode === "off" && !draft.allowNetwork && !draft.dockerImage && draft.extraReadPaths.length === 0;

  const liveCount = runningAgents(Object.values(agents), { project });
  const restartMsg = restartNote(liveCount);

  const save = async () => {
    if (!settings || saving) return;
    setSaving(true);
    try {
      const key = normProject(project);
      const nextMap: Record<string, ProjectSandboxConfig> = {};
      for (const [k, v] of Object.entries(settings.projectSandboxes ?? {})) {
        if (normProject(k) !== key) nextMap[k] = v;
      }
      if (!isDefault) nextMap[key] = draft;
      const updated = await attempt("Couldn't save sandbox settings", () =>
        api.updateSettings({ ...settings, projectSandboxes: nextMap }),
      );
      if (updated) {
        toast.success("Sandbox settings saved");
      }
    } finally {
      setSaving(false);
    }
  };

  const showDockerOptions = mode === "auto" || mode === "docker";
  const isLinux = caps ? caps.platform === "linux" : true;
  const missingBackendWarning =
    caps && mode !== "off"
      ? mode === "bwrap" && !caps.bwrapAvailable
        ? isLinux
          ? "Bubblewrap (bwrap) was not found on PATH. Agents will fail to start until bwrap is installed."
          : "Bubblewrap is only available on Linux. Choose Docker or Off on this platform."
        : mode === "docker" && !caps.dockerAvailable
          ? "Docker was not found on PATH. Agents will fail to start until Docker is installed."
          : mode === "auto" && !caps.bwrapAvailable && !caps.dockerAvailable
            ? "Neither Bubblewrap nor Docker was found on PATH. Agents will refuse to start unsandboxed until one is installed."
            : null
      : null;

  return (
    <div className="space-y-4">
      <p className="text-caption leading-relaxed text-muted">
        Isolate unattended agents in this project using Bubblewrap (<code className="font-mono text-fg">bwrap</code>) on
        Linux or Docker containers cross-platform. Filesystem writes are restricted to the agent&rsquo;s worktree and a
        private <code className="font-mono text-fg">/tmp</code>. If a selected backend isn&rsquo;t installed, Kvistcode
        fails cleanly instead of silently running unsandboxed.
      </p>

      <section
        className="overflow-hidden rounded-xl border border-line bg-card edge"
        aria-label="Detected sandbox backends"
      >
        <header className="flex items-center gap-2 border-b border-line px-4 py-2.5 [&_svg]:h-4 [&_svg]:w-4 [&_svg]:text-steel">
          <Container />
          <h3 className="text-body font-semibold text-fg">Detected runtimes</h3>
          <Button
            size="icon-sm"
            variant="ghost"
            className="ml-auto"
            aria-label="Re-check sandbox runtimes"
            disabled={probing}
            onClick={() => void probe()}
          >
            <RefreshCw className={probing ? "animate-spin" : undefined} />
          </Button>
        </header>

        <div className="divide-y divide-line">
          <div className="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
            <div className="min-w-0">
              <div className="text-body font-medium text-fg">Bubblewrap (bwrap)</div>
              <div className="text-micro text-steel">
                {caps?.bwrapPath
                  ? caps.bwrapPath
                  : isLinux
                    ? "Lightweight Linux user-namespace sandbox (recommended on Linux)"
                    : "Linux only"}
              </div>
            </div>
            {probing && !caps ? (
              <Spinner className="h-3.5 w-3.5" />
            ) : caps?.bwrapAvailable ? (
              <Badge tone="ok">Available</Badge>
            ) : (
              <Badge tone="neutral">{isLinux ? "Not installed" : "Linux only"}</Badge>
            )}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
            <div className="min-w-0">
              <div className="text-body font-medium text-fg">Docker</div>
              <div className="text-micro text-steel">
                {caps?.dockerPath ? caps.dockerPath : "Ephemeral container isolation (Linux, macOS, Windows)"}
              </div>
            </div>
            {probing && !caps ? (
              <Spinner className="h-3.5 w-3.5" />
            ) : caps?.dockerAvailable ? (
              <Badge tone="ok">Available</Badge>
            ) : (
              <Badge tone="neutral">Not installed</Badge>
            )}
          </div>
        </div>
      </section>

      <section className="overflow-hidden rounded-xl border border-line bg-card edge" aria-label="Sandbox settings">
        <header className="flex items-center gap-2 border-b border-line px-4 py-2.5 [&_svg]:h-4 [&_svg]:w-4 [&_svg]:text-steel">
          <ShieldCheck />
          <h3 className="text-body font-semibold text-fg">Sandbox policy for this project</h3>
          <span className="ml-auto text-micro text-steel">Saved in Kvistcode settings</span>
        </header>

        <div className="divide-y divide-line">
          <SettingRow
            title="Sandbox mode"
            htmlFor="sandbox-mode"
            description="Auto uses Bubblewrap on Linux when available and falls back to Docker."
          >
            <div className="w-56">
              <Select
                id="sandbox-mode"
                aria-label="Sandbox mode"
                value={mode}
                onValueChange={(v) => setMode(v as SandboxMode)}
                options={[
                  { value: "off", label: "Off", description: "Run agents directly on the host" },
                  { value: "auto", label: "Auto (bwrap → Docker)", description: "Prefer Bubblewrap, else Docker" },
                  { value: "bwrap", label: "Bubblewrap", description: "Require bwrap (Linux)" },
                  { value: "docker", label: "Docker", description: "Require Docker container" },
                ]}
              />
            </div>
          </SettingRow>

          <SettingRow
            title="Allow network access"
            htmlFor="sandbox-network"
            description="Off isolates the network (--unshare-net / --network none). Turn on for cloud providers (Claude, Codex, Grok, Gemini) that call remote LLM APIs; filesystem writes stay restricted to the worktree and /tmp."
          >
            <Switch
              id="sandbox-network"
              aria-label="Allow network access"
              checked={allowNetwork}
              disabled={mode === "off"}
              onCheckedChange={setAllowNetwork}
            />
          </SettingRow>

          {showDockerOptions && (
            <div className="space-y-1.5 px-4 py-3.5">
              <Label htmlFor="sandbox-docker-image">Docker image (optional)</Label>
              <Input
                id="sandbox-docker-image"
                className="font-mono text-small"
                placeholder={DEFAULT_DOCKER_IMAGE}
                value={dockerImage}
                onChange={(e) => setDockerImage(e.target.value)}
              />
              <p className="text-micro text-steel">
                Container image used in Docker mode. Leave blank to use{" "}
                <code className="font-mono text-muted">{DEFAULT_DOCKER_IMAGE}</code>.
              </p>
            </div>
          )}

          {mode !== "off" && (
            <div className="space-y-1.5 px-4 py-3.5">
              <Label htmlFor="sandbox-extra-read">Extra read-only host paths (optional)</Label>
              <Textarea
                id="sandbox-extra-read"
                rows={2}
                className="font-mono text-small"
                placeholder={"/opt/toolchains\n~/.cargo"}
                value={extraPathsText}
                onChange={(e) => setExtraPathsText(e.target.value)}
              />
              <p className="text-micro text-steel">
                One absolute path per line. Mounted read-only inside the sandbox. Mounting <code className="font-mono">/</code> is never allowed.
              </p>
            </div>
          )}

          {missingBackendWarning && (
            <div className="flex items-start gap-2 bg-warn/8 px-4 py-3 text-caption text-warn">
              <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>{missingBackendWarning}</span>
            </div>
          )}

          <div className="flex flex-wrap items-center gap-2 px-4 py-3">
            <Button size="sm" disabled={!dirty || saving} onClick={() => void save()}>
              {saving ? <Spinner className="h-3.5 w-3.5 text-current" /> : <Check />}
              Save
            </Button>
            {dirty && (
              <Button size="sm" variant="ghost" disabled={saving} onClick={() => syncForm(savedCfg)}>
                <RotateCcw /> Reset
              </Button>
            )}
            {restartMsg && <span className="ml-auto text-micro text-steel">{restartMsg}</span>}
          </div>
        </div>
      </section>
    </div>
  );
}
