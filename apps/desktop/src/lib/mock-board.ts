import type { BoardCard } from "./types";

/** A few Backlog cards so the board has something in its first column in the browser backend. */
export function seedBoardCards(): BoardCard[] {
  const now = Date.now();
  const card = (i: number, projectPath: string, title: string, notes: string): BoardCard => ({
    id: `card-seed-${i}`,
    projectPath,
    title,
    notes,
    position: i,
    createdAt: now - i * 3_600_000,
    updatedAt: now - i * 3_600_000,
  });
  return [
    card(
      0,
      "/home/sam/Projects/atlas-api",
      "Rate-limit the export endpoint",
      "Return 429 with Retry-After. Add a test.",
    ),
    card(1, "/home/sam/Projects/atlas-api", "Document the webhook retry policy", ""),
    card(2, "/home/sam/Projects/storefront", "Fix the cart badge on mobile", "Badge overlaps the icon below 360px."),
  ];
}
