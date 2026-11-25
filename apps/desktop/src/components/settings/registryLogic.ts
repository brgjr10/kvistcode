import type { CustomAgent, RegistryAgent } from "@/lib/types";

/** Shown before an agent from the registry is added. */
export function registryWarning(a: Pick<RegistryAgent, "name" | "source">): string {
  return `${a.name} is third-party code. Adding it will run ${a.source} on your computer, with your user's permissions, and it can read and change your files. Kvistcode hasn't reviewed it. Only add agents you trust.`;
}

/** Registry agents matching the search, ones that can be added first, then by name. */
export function filterRegistry(list: RegistryAgent[], query: string): RegistryAgent[] {
  const q = query.trim().toLowerCase();
  const hits = q ? list.filter((a) => `${a.name} ${a.id} ${a.description}`.toLowerCase().includes(q)) : list;
  return [...hits].sort(
    (a, b) => Number(a.unsupported !== null) - Number(b.unsupported !== null) || a.name.localeCompare(b.name),
  );
}

/** Whether a custom agent with this name is already set up. */
export function isAdded(entry: RegistryAgent, agents: CustomAgent[]): boolean {
  const name = entry.name.trim().toLowerCase();
  return agents.some((a) => a.name.trim().toLowerCase() === name);
}
