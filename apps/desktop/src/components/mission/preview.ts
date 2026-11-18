/**
 * Plain-text preview of markdown for a tile or a row, keeping the tail of what's streaming.
 * Code blocks, headings, emphasis, links and rules are dropped so "**Done.**" reads as "Done.".
 */
export function plainPreview(text: string, tail = false, max = 220): string {
  // Only a window of the (possibly huge, still streaming) message is ever shown: regex just that.
  const window = text.length > 1200 ? (tail ? text.slice(-400) : text.slice(0, 1200)) : text;
  const plain = window
    .replace(/```[\s\S]*?(```|$)/g, " ")
    .replace(/^\s{0,3}(#{1,6}\s+|>\s?)/gm, "")
    .replace(/[*`]/g, "")
    .replace(/^\s*\|?(\s*:?-+:?\s*\|)+\s*:?-*:?\s*$/gm, " ")
    .replace(/\s*\|\s*/g, " ")
    .replace(/\[(.*?)\]\(.*?\)/g, "$1")
    .replace(/-{3,}/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (plain.length <= max) return plain;
  return tail ? `…${plain.slice(plain.length - max)}` : `${plain.slice(0, max)}…`;
}
