import { LayoutGrid, Plus, Rows3, Search, X } from "lucide-react";
import { useEffect, useState } from "react";
import { BoardPane } from "@/components/board/Board";
import { AllAgentsPane, type Density } from "@/components/mission/AllAgents";
import { NeedsYouPane } from "@/components/mission/NeedsYou";
import { useFleet, type Filter } from "@/components/mission/useFleet";
import { HeaderAction, PageHeader } from "@/components/PageHeader";
import { useTriageCount } from "@/components/triage/useTriage";
import { Segmented } from "@/components/ui/segmented";
import { Tooltip } from "@/components/ui/tooltip";
import { splitCost } from "@/lib/costSplit";
import { formatCost } from "@/lib/format";
import { isMac, modKey } from "@/lib/utils";
import { useApp } from "@/store/app";
import { useUi, type MissionTab } from "@/store/ui";

const DENSITY_KEY = "armada.missionDensity";

function readDensity(): Density {
  try {
    return localStorage.getItem(DENSITY_KEY) === "compact" ? "compact" : "comfortable";
  } catch {
    return "comfortable";
  }
}

/** A tab or filter label with its count; `tone` colours the count while it is above zero. */
function chip(label: string, n: number, tone?: string) {
  return (
    <span className="flex items-center gap-1.5 whitespace-nowrap">
      {label}
      <span
        className="rounded-md bg-base/70 px-1 text-micro tabular text-steel"
        style={tone && n > 0 ? { color: tone } : undefined}
      >
        {n}
      </span>
    </span>
  );
}

/**
 * One place for the fleet: "Needs you" is the queue (answer, approve, resume from here), "All agents"
 * is the live board. Both count agents the same way, so the rail badge, the tab and the title agree.
 */
export function MissionControlView() {
  const needs = useTriageCount();
  const { agents, live, idle } = useFleet();
  const accounts = useApp((s) => s.accounts);
  const openMission = useUi((s) => s.openMission);
  const openNewAgent = useUi((s) => s.openNewAgent);
  // Chosen once when the view opens (Needs you if anything waits), so the tab doesn't jump under you
  // when an agent starts or stops needing you.
  const [opened] = useState<MissionTab>(() => (needs > 0 ? "needs" : "all"));
  const chosen = useUi((s) => s.missionTab);
  const tab = chosen ?? opened;
  useEffect(() => {
    // Record the pick so the shortcuts know which tab is showing.
    if (chosen === null) openMission(opened);
  }, [chosen, opened, openMission]);
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [density, setDensityState] = useState<Density>(readDensity);
  const setDensity = (d: Density) => {
    setDensityState(d);
    try {
      localStorage.setItem(DENSITY_KEY, d);
    } catch {
      /* ignore */
    }
  };

  const cost = splitCost(agents, accounts);

  return (
    <div className="@container flex h-full flex-col">
      <PageHeader
        icon={<LayoutGrid />}
        title="Mission Control"
        subtitle={
          <>
            <span className="tabular">{needs}</span> need{needs === 1 ? "s" : ""} you ·{" "}
            <span className="tabular">{live}</span> live ·{" "}
            {cost.billed > 0 && (
              <>
                <span className="tabular">{formatCost(cost.billed)}</span> billed
                {cost.equivalent > 0 && " · "}
              </>
            )}
            {cost.equivalent > 0 && (
              <>
                <span className="tabular">{formatCost(cost.equivalent)}</span> API-equivalent on subscriptions
              </>
            )}
            {cost.billed === 0 && cost.equivalent === 0 && "no usage yet"}
          </>
        }
        right={
          <>
            {tab === "all" && agents.length > 0 && (
              <Segmented<Density>
                size="sm"
                aria-label="Density"
                value={density}
                onChange={setDensity}
                options={[
                  {
                    value: "comfortable",
                    ariaLabel: "Grid view",
                    label: (
                      <Tooltip content="Tiles">
                        <LayoutGrid className="h-3.5 w-3.5" aria-hidden />
                      </Tooltip>
                    ),
                  },
                  {
                    value: "compact",
                    ariaLabel: "List view",
                    label: (
                      <Tooltip content="Compact rows">
                        <Rows3 className="h-3.5 w-3.5" aria-hidden />
                      </Tooltip>
                    ),
                  },
                ]}
              />
            )}
            <HeaderAction
              variant="primary"
              icon={<Plus />}
              label="New agent"
              hint={`${modKey}${isMac ? "" : "+"}N`}
              onClick={() => openNewAgent()}
            />
          </>
        }
      >
        <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
          <Segmented<MissionTab>
            size="sm"
            aria-label="Mission Control"
            value={tab}
            onChange={openMission}
            options={[
              { value: "needs", label: chip("Needs you", needs, "var(--warn)") },
              { value: "all", label: chip("All agents", agents.length) },
              { value: "board", label: "Board" },
            ]}
          />
          {tab === "all" && agents.length > 0 && (
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <FleetSearch value={query} onChange={setQuery} />
              <Segmented<Filter>
                size="sm"
                aria-label="Filter agents"
                value={filter}
                onChange={setFilter}
                options={[
                  { value: "all", label: "All" },
                  { value: "live", label: chip("Live", live, "var(--accent)") },
                  { value: "idle", label: chip("Idle", idle) },
                ]}
              />
            </div>
          )}
        </div>
      </PageHeader>

      {tab === "needs" ? (
        <div className="min-h-0 flex-1">
          <NeedsYouPane hasAgents={agents.length > 0} />
        </div>
      ) : tab === "board" ? (
        <div className="flex min-h-0 flex-1 flex-col px-4 py-4 @2xl:px-6 @2xl:py-5">
          <BoardPane />
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto px-4 py-4 @2xl:px-6 @2xl:py-5">
          <AllAgentsPane
            agents={agents}
            filter={filter}
            query={query}
            density={density}
            onClearFilter={() => {
              setFilter("all");
              setQuery("");
            }}
          />
        </div>
      )}
    </div>
  );
}

/** Narrows the board by title, folder, branch or last message (the same match as the sidebar's filter). */
function FleetSearch({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="relative w-44 min-w-0 @2xl:w-56">
      <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-steel" />
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape" && value) {
            e.preventDefault();
            e.stopPropagation();
            onChange("");
          }
        }}
        placeholder="Filter by name or project"
        aria-label="Filter by name or project"
        spellCheck={false}
        className="h-8 w-full rounded-lg border border-line bg-base/50 pl-8 pr-7 text-small text-fg placeholder:text-steel transition-colors hover:border-line-strong focus:border-accent/50 focus:outline-none focus:ring-3 focus:ring-accent/10"
      />
      {value && (
        <button
          type="button"
          aria-label="Clear filter"
          onClick={() => onChange("")}
          className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded p-1 text-steel hover:text-fg"
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </div>
  );
}
