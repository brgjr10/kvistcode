import { CircleDot, GitMerge, GitPullRequest, Pencil, Play, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import { toast } from "sonner";
import { RelTime } from "@/components/RelTime";
import { StatusDot } from "@/components/StatusDot";
import { Button } from "@/components/ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter } from "@/components/ui/dialog";
import { Input, Label, Textarea } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Tooltip } from "@/components/ui/tooltip";
import { api } from "@/lib/api";
import { projectKey } from "@/lib/projectKey";
import type { Agent, BoardCard } from "@/lib/types";
import { basename, cn, errorMessage } from "@/lib/utils";
import { useApp } from "@/store/app";
import { usePr, refreshPr } from "@/store/pr";
import { useUi } from "@/store/ui";
import { boardColumns, type AgentColumn } from "./columns";

const PROJECT_KEY = "kvistcode.boardProject";
const DRAG_TYPE = "application/x-kvistcode-card";

function readProject(): string {
  try {
    return localStorage.getItem(PROJECT_KEY) ?? "";
  } catch {
    return "";
  }
}

/** Opens Mission Control's Board on `path`. */
export function openBoardFor(path: string) {
  try {
    localStorage.setItem(PROJECT_KEY, projectKey(path));
  } catch {
    /* ignore */
  }
  useUi.getState().openMission("board");
}

/** The prompt a card starts an agent with: its title, then its notes. */
export function cardPrompt(c: Pick<BoardCard, "title" | "notes">): string {
  return c.notes ? `${c.title}\n\n${c.notes}` : c.title;
}

/** Projects with an agent or recently opened, most recent first. */
function useProjects(): string[] {
  const agents = useApp((s) => s.agents);
  const recent = useApp((s) => s.settings?.recentProjects);
  return useMemo(() => {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const p of [...(recent ?? []), ...Object.values(agents).map((a) => a.projectPath)]) {
      const k = projectKey(p);
      if (k && !seen.has(k)) {
        seen.add(k);
        out.push(k);
      }
    }
    return out;
  }, [agents, recent]);
}

/** Backlog → Working → Needs review → Merged for one project. */
export function BoardPane() {
  const projects = useProjects();
  const agents = useApp((s) => s.agents);
  const prs = usePr((s) => s.byAgent);
  const openNewAgent = useUi((s) => s.openNewAgent);
  const openAgent = useUi((s) => s.openAgent);
  const [chosen, setChosen] = useState(readProject);
  const project = projects.includes(chosen) ? chosen : (projects[0] ?? "");
  const pick = (p: string) => {
    setChosen(p);
    try {
      localStorage.setItem(PROJECT_KEY, p);
    } catch {
      /* ignore */
    }
  };

  const [cards, setCards] = useState<BoardCard[]>([]);
  const [editing, setEditing] = useState<BoardCard | "new" | null>(null);
  const [dragging, setDragging] = useState(false);
  const loaded = useRef("");

  const load = useCallback(async (p: string) => {
    try {
      const list = await api.boardCards(p);
      if (loaded.current === p) setCards(list);
    } catch (e) {
      toast.error("Couldn't load the backlog", { description: errorMessage(e) });
    }
  }, []);
  useEffect(() => {
    loaded.current = project;
    setCards([]);
    if (project) void load(project);
  }, [project, load]);

  const inProject = useCallback((a: Agent) => projectKey(a.projectPath) === project, [project]);
  const columns = useMemo(() => boardColumns(Object.values(agents), prs, inProject), [agents, prs, inProject]);

  // Merge state comes from the pull request: look once for each agent of this project (cached by the backend).
  const branched = useMemo(
    () =>
      Object.values(agents)
        .filter((a) => inProject(a) && a.branch && !a.parentId)
        .map((a) => a.id)
        .join(","),
    [agents, inProject],
  );
  useEffect(() => {
    for (const id of branched.split(",").filter(Boolean)) void refreshPr(id);
  }, [branched]);

  const start = (c: BoardCard) =>
    openNewAgent({
      projectPath: project,
      prompt: cardPrompt(c),
      title: c.title,
      onLaunched: () => {
        void api
          .deleteBoardCard(c.id)
          .then(() => setCards((list) => list.filter((x) => x.id !== c.id)))
          .catch(() => undefined);
      },
    });

  const remove = async (c: BoardCard) => {
    try {
      await api.deleteBoardCard(c.id);
      setCards((list) => list.filter((x) => x.id !== c.id));
    } catch (e) {
      toast.error("Couldn't delete the card", { description: errorMessage(e) });
    }
  };

  const importIssues = async () => {
    try {
      const { issues } = await api.githubIssues(project);
      const have = new Set(cards.map((c) => c.title));
      let added = 0;
      for (const i of issues) {
        const title = `#${i.number} ${i.title}`;
        if (have.has(title)) continue;
        await api.addBoardCard(project, title, i.url);
        added++;
      }
      await load(project);
      toast.success(added ? `Added ${added} issue${added === 1 ? "" : "s"}` : "No new issues");
    } catch (e) {
      toast.error("Couldn't import issues", { description: errorMessage(e) });
    }
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    const card = cards.find((c) => c.id === e.dataTransfer.getData(DRAG_TYPE));
    if (card) start(card);
  };

  if (projects.length === 0) {
    return <p className="m-auto text-small text-muted">Open a project to plan its work here.</p>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex items-center gap-2">
        <div className="w-60 max-w-full">
          <Select
            aria-label="Project"
            value={project}
            onValueChange={pick}
            options={projects.map((p) => ({ value: p, label: basename(p) || p }))}
          />
        </div>
      </div>
      <div className="grid min-h-0 flex-1 auto-cols-[minmax(240px,1fr)] grid-flow-col gap-3 overflow-x-auto pb-1">
        <Column
          title="Backlog"
          count={cards.length}
          action={
            <>
              <Tooltip content="Import open GitHub issues">
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Import GitHub issues"
                  onClick={() => void importIssues()}
                >
                  <CircleDot className="h-3.5 w-3.5" />
                </Button>
              </Tooltip>
              <Tooltip content="New card">
                <Button variant="ghost" size="icon-sm" aria-label="New card" onClick={() => setEditing("new")}>
                  <Plus className="h-3.5 w-3.5" />
                </Button>
              </Tooltip>
            </>
          }
          empty="Nothing planned. Add a card for work you want to start later."
        >
          {cards.map((c) => (
            <BacklogCard
              key={c.id}
              card={c}
              onStart={() => start(c)}
              onEdit={() => setEditing(c)}
              onDelete={() => void remove(c)}
              onDragState={setDragging}
            />
          ))}
        </Column>
        <Column
          title="Working"
          count={columns.working.length}
          empty="No agents running."
          dropHint={dragging ? "Drop to start" : undefined}
          onDrop={onDrop}
        >
          {columns.working.map((a) => (
            <AgentCard key={a.id} agent={a} column="working" onOpen={openAgent} />
          ))}
        </Column>
        <Column title="Needs review" count={columns.review.length} empty="Nothing waiting on you.">
          {columns.review.map((a) => (
            <AgentCard key={a.id} agent={a} column="review" onOpen={openAgent} />
          ))}
        </Column>
        <Column title="Merged" count={columns.merged.length} empty="Merged work shows up here.">
          {columns.merged.map((a) => (
            <AgentCard key={a.id} agent={a} column="merged" onOpen={openAgent} />
          ))}
        </Column>
      </div>
      {editing && (
        <CardDialog
          card={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSave={async (title, notes) => {
            if (editing === "new") await api.addBoardCard(project, title, notes);
            else await api.updateBoardCard(editing.id, title, notes);
            await load(project);
          }}
        />
      )}
    </div>
  );
}

function Column({
  title,
  count,
  action,
  empty,
  dropHint,
  onDrop,
  children,
}: {
  title: string;
  count: number;
  action?: React.ReactNode;
  empty: string;
  dropHint?: string;
  onDrop?: (e: DragEvent) => void;
  children: React.ReactNode;
}) {
  const [over, setOver] = useState(false);
  return (
    <section
      aria-label={title}
      onDragOver={
        onDrop
          ? (e) => {
              if (e.dataTransfer.types.includes(DRAG_TYPE)) {
                e.preventDefault();
                setOver(true);
              }
            }
          : undefined
      }
      onDragLeave={onDrop ? () => setOver(false) : undefined}
      onDrop={
        onDrop
          ? (e) => {
              setOver(false);
              onDrop(e);
            }
          : undefined
      }
      className={cn(
        "flex min-h-0 min-w-0 flex-col rounded-xl border border-line bg-panel/40",
        over && "border-accent/60 bg-accent/5",
      )}
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 px-3">
        <h2 className="flex items-center gap-2 text-small font-medium text-fg">
          {title}
          <span className="rounded-md bg-base/70 px-1 text-micro tabular text-steel">{count}</span>
        </h2>
        <div className="flex items-center gap-0.5">{action}</div>
      </header>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-2">
        {count === 0 && <p className="px-1 py-2 text-small text-steel">{dropHint ?? empty}</p>}
        {count > 0 && dropHint && <p className="px-1 text-small text-accent">{dropHint}</p>}
        {children}
      </div>
    </section>
  );
}

function BacklogCard({
  card,
  onStart,
  onEdit,
  onDelete,
  onDragState,
}: {
  card: BoardCard;
  onStart: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onDragState: (on: boolean) => void;
}) {
  return (
    <article
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData(DRAG_TYPE, card.id);
        e.dataTransfer.effectAllowed = "move";
        onDragState(true);
      }}
      onDragEnd={() => onDragState(false)}
      className="rounded-lg border border-line bg-card p-2.5 edge"
    >
      <h3 className="text-small font-medium text-fg">{card.title}</h3>
      {card.notes && (
        <p className="mt-1 line-clamp-3 whitespace-pre-wrap break-words text-caption text-muted">{card.notes}</p>
      )}
      <div className="mt-2 flex items-center gap-1">
        <Button size="xs" variant="outline" onClick={onStart}>
          <Play className="h-3 w-3" aria-hidden />
          Start
        </Button>
        <span className="flex-1" />
        <Button size="icon-xs" variant="ghost" aria-label={`Edit ${card.title}`} onClick={onEdit}>
          <Pencil className="h-3 w-3" />
        </Button>
        <Button size="icon-xs" variant="ghost" aria-label={`Delete ${card.title}`} onClick={onDelete}>
          <Trash2 className="h-3 w-3" />
        </Button>
      </div>
    </article>
  );
}

function AgentCard({ agent, column, onOpen }: { agent: Agent; column: AgentColumn; onOpen: (id: string) => void }) {
  const pr = usePr((s) => s.byAgent[agent.id]?.pr);
  return (
    <button
      type="button"
      onClick={() => onOpen(agent.id)}
      className="w-full rounded-lg border border-line bg-card p-2.5 text-left edge transition-colors hover:border-line-strong focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-accent/30"
    >
      <span className="flex items-start gap-2">
        <span className="mt-1.5">
          <StatusDot status={agent.status} size={6} />
        </span>
        <span className="min-w-0 flex-1 truncate text-small font-medium text-fg">{agent.title || "Untitled"}</span>
      </span>
      {agent.lastMessage && column !== "merged" && (
        <span className="mt-1 line-clamp-2 block break-words text-caption text-muted">{agent.lastMessage}</span>
      )}
      <span className="mt-1.5 flex items-center gap-2 text-micro text-steel">
        {pr ? (
          <span className={cn("inline-flex items-center gap-1", pr.state === "merged" && "text-ok")}>
            {pr.state === "merged" ? <GitMerge className="h-3 w-3" /> : <GitPullRequest className="h-3 w-3" />}#
            {pr.number}
          </span>
        ) : (
          agent.branch && <span className="truncate">{agent.branch}</span>
        )}
        <span className="flex-1" />
        <RelTime ts={agent.lastActivityAt} />
      </span>
    </button>
  );
}

function CardDialog({
  card,
  onClose,
  onSave,
}: {
  card: BoardCard | null;
  onClose: () => void;
  onSave: (title: string, notes: string) => Promise<void>;
}) {
  const [title, setTitle] = useState(card?.title ?? "");
  const [notes, setNotes] = useState(card?.notes ?? "");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!title.trim() || busy) return;
    setBusy(true);
    try {
      await onSave(title, notes);
      onClose();
    } catch (e) {
      toast.error("Couldn't save the card", { description: errorMessage(e) });
      setBusy(false);
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={card ? "Edit card" : "New card"}
        description="Work to start later. Notes become the agent's prompt."
      >
        <DialogBody>
          <div className="space-y-3">
            <div>
              <Label htmlFor="card-title">Title</Label>
              <Input
                id="card-title"
                autoFocus
                value={title}
                maxLength={200}
                onChange={(e) => setTitle(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && void submit()}
              />
            </div>
            <div>
              <Label htmlFor="card-notes">Notes (optional)</Label>
              <Textarea id="card-notes" value={notes} onChange={(e) => setNotes(e.target.value)} />
            </div>
          </div>
        </DialogBody>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" disabled={!title.trim() || busy} onClick={() => void submit()}>
            {card ? "Save" : "Add card"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
