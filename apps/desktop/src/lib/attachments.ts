import type { Attachment } from "./types";

export const MAX_ATTACHMENTS = 8;
export const MAX_ATTACHMENT_BYTES = 5 * 1024 * 1024;
/** Formats every provider accepts; SVG/TIFF/HEIC etc. are rejected with a clear message. */
export const ACCEPTED_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
export const ACCEPTED_IMAGE_LABEL = "PNG, JPEG, GIF or WebP";

export function attachmentSrc(a: Attachment): string {
  return `data:${a.mimeType};base64,${a.data ?? ""}`;
}

function readAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onerror = () => reject(r.error ?? new Error(`Couldn't read ${file.name}`));
    r.onload = () => {
      const url = String(r.result ?? "");
      resolve(url.slice(url.indexOf(",") + 1));
    };
    r.readAsDataURL(file);
  });
}

/**
 * Validate and read image files. Returns the attachments that fit the limits
 * (images only, 5 MB each, 8 total) and human-readable reasons for the rest.
 */
export async function filesToAttachments(
  files: Iterable<File>,
  existing: number,
): Promise<{ attachments: Attachment[]; problems: string[] }> {
  const attachments: Attachment[] = [];
  const problems: string[] = [];
  let room = MAX_ATTACHMENTS - existing;
  for (const file of files) {
    if (!file.type.startsWith("image/")) {
      problems.push(`${file.name || "File"} isn't an image`);
      continue;
    }
    if (!ACCEPTED_IMAGE_TYPES.includes(file.type)) {
      const fmt = file.type
        .replace("image/", "")
        .replace(/\+xml$/, "")
        .toUpperCase();
      problems.push(`${file.name || "Image"} is ${fmt}: use ${ACCEPTED_IMAGE_LABEL}`);
      continue;
    }
    if (file.size > MAX_ATTACHMENT_BYTES) {
      problems.push(`${file.name || "Image"} is larger than 5 MB`);
      continue;
    }
    if (room <= 0) {
      problems.push(`Only ${MAX_ATTACHMENTS} images per message; the rest weren't attached`);
      break;
    }
    try {
      const data = await readAsBase64(file);
      const ext = file.type.split("/")[1] ?? "png";
      attachments.push({ name: file.name || `pasted-image.${ext}`, mimeType: file.type, data });
      room--;
    } catch {
      problems.push(`Couldn't read ${file.name || "image"}`);
    }
  }
  return { attachments, problems };
}

/** True when a paste also carries text (e.g. a copied web snippet with an image). */
export function hasText(dt: DataTransfer | null): boolean {
  if (!dt) return false;
  const types = Array.from(dt.types ?? []);
  return types.includes("text/plain") || types.includes("text/html");
}

/** True for a paste of files copied in a file manager (uri-list / gnome-copied-files). */
export function isFileListPaste(dt: DataTransfer | null): boolean {
  if (!dt) return false;
  const types = Array.from(dt.types ?? []);
  return types.includes("text/uri-list") || types.includes("x-special/gnome-copied-files");
}

/** Image files from a paste or drop event. */
export function imageFilesFrom(dt: DataTransfer | null): File[] {
  if (!dt) return [];
  const out: File[] = [];
  for (const item of Array.from(dt.items ?? [])) {
    if (item.kind === "file") {
      const f = item.getAsFile();
      if (f) out.push(f);
    }
  }
  if (out.length === 0) out.push(...Array.from(dt.files ?? []));
  return out;
}

export function hasFiles(dt: DataTransfer | null): boolean {
  return !!dt && Array.from(dt.types ?? []).includes("Files");
}

/** What makes two attachments the same picture: its bytes, or the file it points at. */
function sameKey(a: Attachment): string | null {
  if (a.data) return `data:${a.mimeType}:${a.data}`;
  if (a.path) return `path:${a.path}`;
  return null;
}

/**
 * Adds `incoming` after `current`: a picture that is already attached (or comes twice) is skipped,
 * so dropping or pasting the same image again doesn't add it twice, and the total stays within MAX_ATTACHMENTS.
 */
export function mergeAttachments(
  current: Attachment[],
  incoming: Attachment[],
): { next: Attachment[]; duplicates: number; dropped: number } {
  const seen = new Set<string>();
  for (const a of current) {
    const k = sameKey(a);
    if (k) seen.add(k);
  }
  const fresh: Attachment[] = [];
  let duplicates = 0;
  for (const a of incoming) {
    const k = sameKey(a);
    if (k && seen.has(k)) {
      duplicates++;
      continue;
    }
    if (k) seen.add(k);
    fresh.push(a);
  }
  const room = Math.max(0, MAX_ATTACHMENTS - current.length);
  const accepted = fresh.slice(0, room);
  return { next: [...current, ...accepted], duplicates, dropped: fresh.length - accepted.length };
}

/** Why some pictures weren't added (empty when all of them were). */
export function skippedNotes(duplicates: number, dropped: number): string[] {
  const out: string[] = [];
  if (duplicates > 0)
    out.push(duplicates === 1 ? "That image is already attached" : `${duplicates} images were already attached`);
  if (dropped > 0) out.push(`Only ${MAX_ATTACHMENTS} images per message; ${dropped} not attached`);
  return out;
}
