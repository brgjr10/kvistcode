import type { TimelineItem } from "./types";

/** Pastes bigger than this become a chip instead of filling the message box. */
export const BIG_PASTE_CHARS = 32 * 1024;
export const BIG_PASTE_LINES = 600;
/** Longer sent messages are left out of the Up-arrow history (they would flood the box). */
const MAX_HISTORY_CHARS = 20_000;
const MAX_HISTORY = 100;

export interface BigPaste {
  id: number;
  text: string;
}

export function isBigPaste(text: string): boolean {
  if (text.length > BIG_PASTE_CHARS) return true;
  let lines = 1;
  for (let i = text.indexOf("\n"); i !== -1; i = text.indexOf("\n", i + 1)) if (++lines > BIG_PASTE_LINES) return true;
  return false;
}

export function lineCount(text: string): number {
  return text.split("\n").length;
}

export function sizeLabel(chars: number): string {
  return chars >= 1024 * 1024
    ? `${(chars / 1024 / 1024).toFixed(1)} MB`
    : `${Math.max(1, Math.round(chars / 1024))} KB`;
}

/** The message with each big paste appended as a fenced block (the fence is longer than any run of backticks inside). */
export function withPastes(message: string, pastes: BigPaste[]): string {
  const blocks = pastes.map((p) => {
    const longest = Math.max(2, ...(p.text.match(/`+/g) ?? []).map((m) => m.length));
    const fence = "`".repeat(longest + 1);
    return `${fence}\n${p.text.replace(/\n$/, "")}\n${fence}`;
  });
  return [message, ...blocks].filter((s) => s.length > 0).join("\n\n");
}

/** What you sent this agent, oldest first: for Up/Down in an empty message box. */
export function sentHistory(items: readonly TimelineItem[] | undefined): string[] {
  const out: string[] = [];
  for (const it of items ?? []) {
    if (it.kind !== "user_message" || it.automatic) continue;
    const t = it.text.trim();
    if (!t || t.length > MAX_HISTORY_CHARS || out[out.length - 1] === t) continue;
    out.push(t);
  }
  return out.slice(-MAX_HISTORY);
}

/** "> " in front of every line, then a blank line so you can type your reply under it. */
export function quoteBlock(text: string): string {
  const body = text.trim().replace(/\r\n?/g, "\n");
  return `${body
    .split("\n")
    .map((l) => (l ? `> ${l}` : ">"))
    .join("\n")}\n\n`;
}

/** Caret on the first line of the text (nothing selected). */
export function caretOnFirstLine(text: string, start: number, end: number): boolean {
  return start === end && !text.slice(0, start).includes("\n");
}

export function caretOnLastLine(text: string, start: number, end: number): boolean {
  return start === end && !text.slice(end).includes("\n");
}
