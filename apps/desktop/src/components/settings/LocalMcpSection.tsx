import { KeyRound, RefreshCw, Server, ShieldAlert, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { CopyButton } from "@/components/CopyButton";
import { ConfirmDialog } from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge, Spinner } from "@/components/ui/misc";
import { Switch } from "@/components/ui/switch";
import { api } from "@/lib/api";
import type { LocalMcpSettings, LocalMcpStatus } from "@/lib/types";
import { cn, errorMessage } from "@/lib/utils";
import { useApp } from "@/store/app";
import { FieldError } from "./FieldError";
import { SettingGroup, SettingRow } from "./SettingRow";
import { fieldErrorId, saveSettingsPatch, useFieldError } from "./settingsErrors";
import { useCommitOnUnmount } from "./useCommitOnUnmount";

const DEFAULT_LOCAL_MCP: LocalMcpSettings = {
  enabled: false,
  port: 47822,
  allowReadTools: true,
  allowWriteTools: false,
  exposeToAgents: false,
};

function validPort(raw: string): number | null {
  if (!/^\d+$/.test(raw.trim())) return null;
  const n = Number(raw.trim());
  return n >= 1024 && n <= 65535 ? n : null;
}

/** Settings → Projects → MCP: opt-in loopback MCP server exposing Kvistcode's agent fleet. */
export function LocalMcpSection() {
  const settings = useApp((s) => s.settings);
  const cfg: LocalMcpSettings = { ...DEFAULT_LOCAL_MCP, ...(settings?.localMcp ?? {}) };

  const [status, setStatus] = useState<LocalMcpStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [rotating, setRotating] = useState(false);
  const [confirmRotate, setConfirmRotate] = useState(false);
  const [showConfig, setShowConfig] = useState(false);
  const [port, setPort] = useState(String(cfg.port));

  const keepTyped = useRef(false);
  useEffect(() => {
    if (keepTyped.current) return;
    setPort(String(cfg.port));
  }, [cfg.port]);

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.localMcpStatus());
      setLoadError(null);
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const apply = async (patch: Partial<LocalMcpSettings>) => {
    const current = useApp.getState().settings;
    if (!current) return;
    const base: LocalMcpSettings = { ...DEFAULT_LOCAL_MCP, ...(current.localMcp ?? {}) };
    setApplying(true);
    keepTyped.current = "port" in patch;
    const ok = await saveSettingsPatch({ localMcp: { ...base, ...patch } }, (message) =>
      toast.error("Couldn't update local MCP server", { description: message }),
    );
    setTimeout(() => (keepTyped.current = !ok && "port" in patch), 0);
    if (ok) {
      await refresh();
    }
    setApplying(false);
  };

  const portBackendError = useFieldError("localMcpPort");
  const parsedPort = validPort(port);
  const portError = parsedPort === null ? "Use a port between 1024 and 65535." : null;
  const commitPort = () => {
    if (parsedPort === null || parsedPort === cfg.port) return;
    void apply({ port: parsedPort });
  };
  useCommitOnUnmount(commitPort);

  const rotate = async () => {
    setRotating(true);
    try {
      setStatus(await api.localMcpRotateToken());
      toast.success("Local MCP token rotated", {
        description: "Update any external MCP clients with the new config snippet.",
      });
    } catch (e) {
      toast.error("Couldn't rotate local MCP token", { description: errorMessage(e) });
    } finally {
      setRotating(false);
      setConfirmRotate(false);
    }
  };

  const running = !!status?.running;
  const serverError = status?.error ?? loadError;

  return (
    <section aria-label="Local MCP server" data-settings-anchor="local-mcp" className="mb-8 scroll-mt-6">
      <div className="mb-2.5 flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Server className="h-4 w-4 text-accent" />
          <h3 className="text-body font-semibold leading-[18px] tracking-tight text-fg">Local MCP server</h3>
          {running ? (
            <Badge tone="ok">Running on 127.0.0.1:{status.port}</Badge>
          ) : serverError ? (
            <Badge tone="err">Error</Badge>
          ) : (
            <Badge>Off</Badge>
          )}
        </div>
        {applying && (
          <span className="flex items-center gap-1.5 text-caption text-steel" role="status">
            <Spinner className="h-3.5 w-3.5" /> Applying…
          </span>
        )}
      </div>
      <p className="mb-3 text-caption leading-[18px] text-muted">
        Expose Kvistcode&apos;s agent fleet on loopback only (<span className="font-mono">127.0.0.1</span>) so external
        CLI agents or sibling Kvistcode agents can inspect diffs, read summaries, or delegate tasks.
      </p>

      <SettingGroup>
        <SettingRow
          title="Enable local MCP server"
          description="Listens on 127.0.0.1 only and requires a bearer token on every request."
          htmlFor="local-mcp-enabled"
        >
          <Switch
            id="local-mcp-enabled"
            checked={cfg.enabled}
            disabled={applying}
            onCheckedChange={(v) => void apply({ enabled: v })}
          />
        </SettingRow>

        <SettingRow
          title="Loopback port"
          description={portError ?? "Bound strictly to 127.0.0.1 (never exposed on your network)."}
          htmlFor="local-mcp-port"
        >
          <div className="flex flex-col items-end">
            <Input
              id="local-mcp-port"
              inputMode="numeric"
              value={port}
              disabled={applying}
              aria-invalid={!!portError || !!portBackendError}
              aria-describedby={portBackendError ? fieldErrorId("localMcpPort") : undefined}
              onChange={(e) => setPort(e.target.value)}
              onBlur={commitPort}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitPort();
                if (e.key === "Escape") {
                  keepTyped.current = false;
                  setPort(String(cfg.port));
                }
              }}
              className={cn(
                "h-8 w-[96px] text-right font-mono text-small tabular",
                (portError || portBackendError) && "border-err/60 focus:border-err/70 focus:ring-err/15",
              )}
            />
            <FieldError field="localMcpPort" />
          </div>
        </SettingRow>

        <SettingRow
          title="Read tools"
          description="Allows list_agents, get_agent_status, get_diff and read_summary (secrets redacted)."
          htmlFor="local-mcp-read"
        >
          <Switch
            id="local-mcp-read"
            checked={cfg.allowReadTools}
            disabled={applying}
            onCheckedChange={(v) => void apply({ allowReadTools: v })}
          />
        </SettingRow>

        <SettingRow
          title="Write tools"
          description="Allows spawn_agent and send_message so MCP clients can start or message Kvistcode agents."
          htmlFor="local-mcp-write"
        >
          <Switch
            id="local-mcp-write"
            checked={cfg.allowWriteTools}
            disabled={applying}
            onCheckedChange={(v) => void apply({ allowWriteTools: v })}
          />
        </SettingRow>

        <SettingRow
          title="Auto-connect Kvistcode agents"
          description="Automatically gives newly started or resumed Kvistcode agents access to this local MCP server."
          htmlFor="local-mcp-expose"
        >
          <Switch
            id="local-mcp-expose"
            checked={cfg.exposeToAgents}
            disabled={applying}
            onCheckedChange={(v) => void apply({ exposeToAgents: v })}
          />
        </SettingRow>
      </SettingGroup>

      {serverError && (
        <div
          role="alert"
          className="mt-3 flex items-center gap-3 rounded-xl border border-err/30 bg-err/[0.06] px-4 py-3"
        >
          <ShieldAlert className="h-4 w-4 shrink-0 text-err" />
          <p className="min-w-0 flex-1 text-small text-err">{serverError}</p>
          <Button size="sm" variant="secondary" onClick={() => void refresh()}>
            <RefreshCw /> Retry
          </Button>
        </div>
      )}

      {cfg.enabled && status && (
        <div className="mt-3 rounded-xl border border-line bg-card/60 px-4 py-3.5 edge">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="min-w-0 space-y-1">
              <div className="flex flex-wrap items-center gap-2 text-small">
                <ShieldCheck className="h-4 w-4 shrink-0 text-accent" />
                <span className="font-mono text-fg">{status.url ?? `http://127.0.0.1:${status.port}/mcp`}</span>
                {status.tokenPreview && (
                  <span className="rounded border border-line bg-base/60 px-1.5 py-0.5 font-mono text-micro text-muted">
                    Bearer {status.tokenPreview}
                  </span>
                )}
              </div>
              <p className="text-caption text-steel">
                Active tools:{" "}
                {status.toolsEnabled.length > 0 ? (
                  <span className="font-mono text-muted">{status.toolsEnabled.join(", ")}</span>
                ) : (
                  "none (enable Read or Write tools above)"
                )}
              </p>
            </div>
            <div className="flex shrink-0 flex-wrap items-center gap-1.5">
              {status.configSnippet && (
                <CopyButton
                  text={status.configSnippet}
                  label="Copy MCP config"
                  className="h-8 rounded-lg border border-line bg-raised/70 px-2.5 text-caption text-fg hover:bg-raised"
                />
              )}
              <Button
                size="sm"
                variant="ghost"
                disabled={rotating}
                onClick={() => setConfirmRotate(true)}
              >
                {rotating ? <Spinner className="h-3.5 w-3.5" /> : <KeyRound />} Rotate token
              </Button>
            </div>
          </div>

          {status.configSnippet && (
            <div className="mt-3 border-t border-line/70 pt-2.5">
              <button
                type="button"
                onClick={() => setShowConfig((v) => !v)}
                className="text-caption font-medium text-muted transition-colors hover:text-fg"
              >
                {showConfig ? "Hide client config JSON" : "Show client config JSON (Claude Code, Codex, Cursor)"}
              </button>
              {showConfig && (
                <pre className="mt-2 overflow-x-auto rounded-lg border border-line bg-base/70 p-3 font-mono text-micro leading-[16px] text-fg/90">
                  {status.configSnippet}
                </pre>
              )}
            </div>
          )}
        </div>
      )}

      <ConfirmDialog
        open={confirmRotate}
        onOpenChange={setConfirmRotate}
        danger
        busy={rotating}
        title="Rotate local MCP bearer token?"
        description="Existing external clients will be rejected immediately until you paste the new config. Running Kvistcode agents will pick up the new token when restarted or resumed."
        confirmLabel="Rotate token"
        onConfirm={rotate}
      />
    </section>
  );
}
