// Sandboxed inline previews of HTML/SVG code blocks. The security model lives here:
//  - in the app the page is served by the `kvartifact:` scheme (`src-tauri/src/artifact_protocol.rs`)
//    into `<iframe sandbox="allow-scripts">` (opaque origin, no same-origin, no top navigation, popups
//    or forms). A `data:` or `srcdoc` frame would inherit the app's own CSP, which forbids inline
//    scripts, so it could not run anything. The browser mock has no such CSP and uses `data:`;
//  - the page carries its own CSP (header and meta): no network at all, inline script/style,
//    data:/blob: images;
//  - SVG is shown through `<img>`, where scripts never run.

/** Largest artifact we will render (bytes of source). Bigger ones stay code only. */
export const ARTIFACT_MAX_BYTES = 512 * 1024;
export const ARTIFACT_MIN_HEIGHT = 48;
export const ARTIFACT_MAX_HEIGHT = 640;
export const ARTIFACT_DEFAULT_HEIGHT = 240;

export type ArtifactKind = "html" | "svg";

/** No network (no connect/img/font/frame/form), nothing but inline script and style. */
export const ARTIFACT_CSP =
  "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; base-uri 'none'; form-action 'none'";

export const ARTIFACT_SANDBOX = "allow-scripts";

const DOCUMENT_RE = /^\s*(<!doctype\s+html|<html[\s>])/i;
const SVG_RE = /^\s*(<\?xml[^>]*\?>\s*)?(<!--[\s\S]*?-->\s*)*<svg[\s>]/i;

/** Which previewable kind a fenced block is, or null. Looks at the tag first, then at untagged whole documents. */
export function artifactKind(lang: string | null, code: string): ArtifactKind | null {
  const l = (lang ?? "").trim().toLowerCase();
  if (l === "svg") return SVG_RE.test(code) ? "svg" : null;
  if (l === "html" || l === "htm") return code.trim() ? "html" : null;
  if (l === "" || l === "text" || l === "xml") {
    if (DOCUMENT_RE.test(code)) return "html";
    if (SVG_RE.test(code)) return "svg";
  }
  return null;
}

/** A whole page (doctype or <html>) opens on Preview; short snippets and SVGs open on Code. */
export function previewByDefault(kind: ArtifactKind, code: string): boolean {
  return kind === "html" && DOCUMENT_RE.test(code);
}

export function byteLength(s: string): number {
  return new TextEncoder().encode(s).length;
}

export function withinCap(code: string): boolean {
  // Cheap bounds first: a UTF-16 unit is at most 3 UTF-8 bytes and at least 1.
  if (code.length * 3 <= ARTIFACT_MAX_BYTES) return true;
  if (code.length > ARTIFACT_MAX_BYTES) return false;
  return byteLength(code) <= ARTIFACT_MAX_BYTES;
}

/** Runs first in the page: drops any Tauri bridge the webview injected, then reports the height. */
function guardScript(id: string): string {
  // `<` is escaped so an id can never close the script tag.
  const safeId = JSON.stringify(id).replace(/</g, "\\u003c");
  return `<script>(function(){try{["__TAURI__","__TAURI_INTERNALS__","__TAURI_IPC__","ipc"].forEach(function(k){try{delete window[k]}catch(e){}try{window[k]=undefined}catch(e){}})}catch(e){}
var id=${safeId},last=0;
function send(){var d=document.documentElement,h=Math.ceil(Math.max(d.scrollHeight,document.body?document.body.scrollHeight:0));if(h===last)return;last=h;parent.postMessage({kvArtifact:1,id:id,height:h},"*")}
addEventListener("load",send);try{new ResizeObserver(send).observe(document.documentElement)}catch(e){}setTimeout(send,50)})();</script>`;
}

/** The full page handed to the iframe: CSP meta first, guard script, then the agent's markup. */
export function buildArtifactDoc(code: string, id: string): string {
  const head = `<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${ARTIFACT_CSP}"><meta name="color-scheme" content="light dark">${guardScript(id)}`;
  // The CSP must come before anything of the agent's: put it right after the opening <head>/<html>,
  // or ahead of the whole fragment when there is neither.
  const m = /<head(\s[^>]*)?>/i.exec(code) ?? /<html(\s[^>]*)?>/i.exec(code);
  if (m) {
    const at = m.index + m[0].length;
    const bare = !/^<head/i.test(m[0]);
    return code.slice(0, at) + (bare ? `<head>${head}</head>` : head) + code.slice(at);
  }
  return `<!doctype html><html><head>${head}<style>html,body{margin:0}body{padding:8px;font:14px system-ui,sans-serif}</style></head><body>${code}</body></html>`;
}

/** Page that shows an SVG via <img> (scripts in it never run), centred and scaled down to fit. */
export function buildSvgDoc(svg: string, id: string): string {
  const uri = `data:image/svg+xml;base64,${toBase64(svg)}`;
  return buildArtifactDoc(
    `<style>img{display:block;max-width:100%;height:auto;margin:0 auto}</style><img alt="" src="${uri}">`,
    id,
  );
}

export function toBase64(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

export const ARTIFACT_SCHEME = "kvartifact";

type TauriInternals = { convertFileSrc?: (path: string, protocol: string) => string };

/** `src` for the sandboxed iframe: the `kvartifact:` scheme in the app, a `data:` URL in a browser. */
export function artifactSrc(kind: ArtifactKind, code: string, id: string): string {
  const doc = kind === "svg" ? buildSvgDoc(code, id) : buildArtifactDoc(code, id);
  const b64 = toBase64(doc);
  const tauri = (globalThis as { __TAURI_INTERNALS__?: TauriInternals }).__TAURI_INTERNALS__;
  if (tauri?.convertFileSrc) {
    const url = b64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    return tauri.convertFileSrc(url, ARTIFACT_SCHEME);
  }
  return `data:text/html;base64,${b64}`;
}

export function clampHeight(h: number): number {
  return Math.min(ARTIFACT_MAX_HEIGHT, Math.max(ARTIFACT_MIN_HEIGHT, Math.round(h)));
}

/**
 * Height from a message event, or null. Accepts only the exact shape our guard script sends, from the
 * frame's own window, carrying this frame's id.
 */
export function parseHeightMessage(
  e: { source: unknown; data: unknown },
  frameWindow: unknown,
  id: string,
): number | null {
  if (!frameWindow || e.source !== frameWindow) return null;
  const d = e.data;
  if (typeof d !== "object" || d === null || Array.isArray(d)) return null;
  const o = d as Record<string, unknown>;
  const keys = Object.keys(o);
  if (keys.length !== 3 || o.kvArtifact !== 1 || o.id !== id) return null;
  if (typeof o.height !== "number" || !Number.isFinite(o.height) || o.height < 0) return null;
  return clampHeight(o.height);
}
