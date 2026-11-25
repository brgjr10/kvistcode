import { Bot, Download, ExternalLink, Search, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/misc";
import { api } from "@/lib/api";
import { openExternal } from "@/lib/external";
import type { CustomAgent, RegistryAgent } from "@/lib/types";
import { errorMessage } from "@/lib/utils";
import { filterRegistry, isAdded, registryWarning } from "./registryLogic";

export const REGISTRY_SITE = "https://agentclientprotocol.com/registry";

type Load = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready"; list: RegistryAgent[] };

function Icon({ src }: { src: string | null }) {
  const [broken, setBroken] = useState(false);
  return (
    <span className="flex h-8 w-8 shrink-0 items-center justify-center overflow-hidden rounded-lg border border-line bg-base/60">
      {src && !broken ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          referrerPolicy="no-referrer"
          onError={() => setBroken(true)}
          className="h-5 w-5 object-contain"
        />
      ) : (
        <Bot className="h-4 w-4 text-steel" />
      )}
    </span>
  );
}

/**
 * Browses the official ACP Registry. The list is fetched each time the dialog opens (nothing is
 * cached or polled); adding asks for confirmation first because the agent is third-party code.
 */
export function RegistryDialog({
  open,
  agents,
  onClose,
  onAdd,
}: {
  open: boolean;
  agents: CustomAgent[];
  onClose: () => void;
  /** Saves the agent; resolves to false when saving failed (it already told the person why). */
  onAdd: (agent: CustomAgent) => Promise<boolean>;
}) {
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<RegistryAgent | null>(null);
  const [adding, setAdding] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!open) return;
    let live = true;
    setLoad({ kind: "loading" });
    setPicked(null);
    setProblem(null);
    api
      .acpRegistryList()
      .then((list) => live && setLoad({ kind: "ready", list }))
      .catch((e) => live && setLoad({ kind: "error", message: errorMessage(e) }));
    return () => {
      live = false;
    };
  }, [open, attempt]);

  const add = async (entry: RegistryAgent) => {
    setAdding(true);
    setProblem(null);
    try {
      const prepared = await api.acpRegistryPrepare(entry.id, entry.version);
      if (await onAdd(prepared)) onClose();
    } catch (e) {
      setProblem(errorMessage(e));
    } finally {
      setAdding(false);
    }
  };

  const shown = load.kind === "ready" ? filterRegistry(load.list, query) : [];

  return (
    <Dialog open={open} onOpenChange={(o) => !o && !adding && onClose()}>
      <DialogContent
        title={picked ? `Add ${picked.name}?` : "Add from ACP Registry"}
        description={
          picked
            ? "Check what will run before you add it."
            : "Agents published in the official Agent Client Protocol registry."
        }
        className="w-[min(640px,calc(100vw-32px))]"
      >
        {picked ? (
          <>
            <DialogBody className="space-y-3">
              <div
                role="alert"
                className="flex gap-2 rounded-lg border border-warn/40 bg-warn/10 p-3 text-body text-fg"
              >
                <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-warn" />
                <span>{registryWarning(picked)}</span>
              </div>
              <dl className="grid grid-cols-[72px_1fr] gap-x-3 gap-y-2 text-meta">
                <dt className="text-steel">Source</dt>
                <dd className="text-fg">{picked.source}</dd>
                <dt className="text-steel">Version</dt>
                <dd className="text-fg">{picked.version || "unknown"}</dd>
                <dt className="text-steel">Runs</dt>
                <dd className="break-words font-mono text-micro text-fg/90">{picked.runs}</dd>
                {picked.license && (
                  <>
                    <dt className="text-steel">License</dt>
                    <dd className="text-fg">{picked.license}</dd>
                  </>
                )}
              </dl>
              {(picked.repository || picked.website) && (
                <button
                  type="button"
                  onClick={() => void openExternal((picked.repository ?? picked.website) as string)}
                  className="inline-flex items-center gap-1 text-meta text-muted underline decoration-line-strong underline-offset-2 hover:text-fg"
                >
                  {picked.repository ? "View its code" : "Visit its website"} <ExternalLink className="h-3 w-3" />
                </button>
              )}
              {problem && (
                <p role="alert" className="text-meta text-err">
                  {problem}
                </p>
              )}
            </DialogBody>
            <DialogFooter>
              <Button variant="ghost" disabled={adding} onClick={() => setPicked(null)}>
                Back
              </Button>
              <Button disabled={adding} onClick={() => void add(picked)}>
                {adding ? <Spinner className="h-3.5 w-3.5" /> : <Download />}
                {adding ? "Adding…" : "I trust it, add it"}
              </Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <DialogBody className="space-y-3">
              <div className="relative">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-steel" />
                <Input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Search agents"
                  aria-label="Search agents"
                  className="pl-8"
                />
              </div>
              {load.kind === "loading" && (
                <p className="flex items-center gap-2 py-6 text-body text-muted">
                  <Spinner className="h-3.5 w-3.5" /> Loading the registry…
                </p>
              )}
              {load.kind === "error" && (
                <div className="space-y-2 py-4">
                  <p role="alert" className="text-body text-err">
                    {load.message}
                  </p>
                  <Button variant="secondary" size="sm" onClick={() => setAttempt((n) => n + 1)}>
                    Try again
                  </Button>
                </div>
              )}
              {load.kind === "ready" && shown.length === 0 && (
                <p className="py-6 text-body text-muted">No agents match.</p>
              )}
              {load.kind === "ready" && shown.length > 0 && (
                <ul
                  aria-label="ACP Registry agents"
                  className="divide-y divide-line rounded-xl border border-line bg-card"
                >
                  {shown.map((a) => {
                    const added = isAdded(a, agents);
                    return (
                      <li key={a.id} className="flex items-center gap-3 px-3 py-2.5">
                        <Icon src={a.icon} />
                        <span className="min-w-0 flex-1">
                          <span className="flex items-baseline gap-2">
                            <span className="truncate text-body font-medium text-fg">{a.name}</span>
                            <span className="shrink-0 text-micro text-steel">{a.version}</span>
                          </span>
                          <span className="line-clamp-2 text-meta text-muted">{a.description}</span>
                          {a.unsupported && <span className="mt-0.5 block text-micro text-steel">{a.unsupported}</span>}
                        </span>
                        {a.unsupported ? (
                          <span className="shrink-0 text-micro text-steel">Can't add</span>
                        ) : added ? (
                          <span className="shrink-0 text-micro text-steel">Added</span>
                        ) : (
                          <Button size="sm" variant="secondary" onClick={() => setPicked(a)}>
                            Add
                          </Button>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
              <p className="text-micro text-steel">
                Source: the{" "}
                <button
                  type="button"
                  onClick={() => void openExternal(REGISTRY_SITE)}
                  className="underline decoration-line-strong underline-offset-2 hover:text-fg"
                >
                  ACP Registry
                </button>
                , run by the Agent Client Protocol project. Agents that can't be added here need a way to start that
                Kvistcode can't check.
              </p>
            </DialogBody>
            <DialogFooter>
              <Button variant="ghost" onClick={onClose}>
                Close
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
