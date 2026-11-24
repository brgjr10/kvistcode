import { Download } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { api } from "@/lib/api";
import { fullDateTime, pluralize, relativeTime } from "@/lib/format";
import { pickAccount } from "@/lib/launchDefaults";
import type { ExternalSession, Provider } from "@/lib/types";
import { errorMessage } from "@/lib/utils";
import { useApp } from "@/store/app";
import { useUi } from "@/store/ui";
import { Button } from "./ui/button";
import { Dialog, DialogBody, DialogContent } from "./ui/dialog";
import { Spinner } from "./ui/misc";
import { Segmented } from "./ui/segmented";

type Source = "claude" | "codex";

/** "Terminal sessions" button: lists Claude Code / Codex sessions started in a terminal in this project. */
export function ImportSessionsButton({ project, onDone }: { project: string; onDone: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button
        variant="secondary"
        className="h-9"
        disabled={!project.trim()}
        title="Continue a session you started in a terminal"
        onClick={() => setOpen(true)}
      >
        <Download />
        Terminal sessions
      </Button>
      {open && <ImportSessionsDialog project={project.trim()} onClose={() => setOpen(false)} onDone={onDone} />}
    </>
  );
}

function ImportSessionsDialog({
  project,
  onClose,
  onDone,
}: {
  project: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const accounts = useApp((s) => s.accounts);
  const openAgent = useUi((s) => s.openAgent);
  const [source, setSource] = useState<Source>("claude");
  const [sessions, setSessions] = useState<ExternalSession[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setSessions(null);
    api
      .listExternalSessions(source as Provider, project)
      .then((s) => live && setSessions(s))
      .catch((e: unknown) => {
        if (!live) return;
        setSessions([]);
        toast.error("Couldn't look for terminal sessions", { description: errorMessage(e) });
      });
    return () => {
      live = false;
    };
  }, [source, project]);

  const account = pickAccount(accounts, source);

  const run = async (s: ExternalSession) => {
    if (!account || busy) return;
    setBusy(s.id);
    try {
      const agent = await api.importExternalSession({
        provider: source,
        accountId: account.id,
        projectPath: project,
        sessionId: s.id,
      });
      onClose();
      onDone();
      openAgent(agent.id);
    } catch (e) {
      toast.error("Couldn't import the session", { description: errorMessage(e) });
      setBusy(null);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title="Terminal sessions"
        description="Sessions you started with claude or codex in this project. Kvistcode only reads them."
        className="w-[min(560px,calc(100vw-32px))]"
      >
        <DialogBody>
          <Segmented
            value={source}
            onChange={(v) => setSource(v as Source)}
            options={[
              { value: "claude", label: "Claude Code" },
              { value: "codex", label: "Codex" },
            ]}
          />
          <p className="mt-2 text-meta text-steel">
            Importing starts an agent in this folder that reopens the session. If the CLI can&rsquo;t reopen it, the
            history stays read-only and your next message sends it along as context.
          </p>
          {!account && <p className="mt-2 text-small text-warn">Add a {source} account first.</p>}
          <div className="mt-3 flex flex-col gap-1.5">
            {sessions === null && <Spinner className="mx-auto my-4 h-4 w-4" />}
            {sessions?.length === 0 && (
              <p className="py-4 text-center text-small text-muted">No sessions found here.</p>
            )}
            {sessions?.map((s) => (
              <div key={s.id} className="flex items-center gap-3 rounded-lg border border-line px-3 py-2">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-small text-fg">{s.title || "(no message)"}</div>
                  <div className="text-meta text-steel" title={fullDateTime(s.startedAt)}>
                    {relativeTime(s.startedAt)} &middot; {pluralize(s.messages, "message")}
                  </div>
                </div>
                <Button size="sm" variant="secondary" disabled={!account || !!busy} onClick={() => void run(s)}>
                  {busy === s.id ? <Spinner className="h-3.5 w-3.5 text-current" /> : "Import"}
                </Button>
              </div>
            ))}
          </div>
        </DialogBody>
      </DialogContent>
    </Dialog>
  );
}
