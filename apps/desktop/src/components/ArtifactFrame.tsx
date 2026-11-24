import * as D from "@radix-ui/react-dialog";
import { Maximize2, RefreshCw, X } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import {
  ARTIFACT_DEFAULT_HEIGHT,
  ARTIFACT_SANDBOX,
  artifactSrc,
  parseHeightMessage,
  type ArtifactKind,
} from "@/lib/artifact";

const BTN =
  "inline-flex h-6 w-6 items-center justify-center rounded-md text-steel transition-colors hover:bg-raised hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60";

/** Reload and full-size buttons for the block header. */
export function ArtifactTools({ onReload, onOpen }: { onReload: () => void; onOpen: () => void }) {
  return (
    <>
      <button type="button" className={BTN} onClick={onReload} aria-label="Reload preview" title="Reload preview">
        <RefreshCw className="h-3.5 w-3.5" />
      </button>
      <button type="button" className={BTN} onClick={onOpen} aria-label="Open full size" title="Open full size">
        <Maximize2 className="h-3.5 w-3.5" />
      </button>
    </>
  );
}

/**
 * The sandboxed page. Mounted only while the block is near the viewport (and unmounted once far away,
 * keeping its last height), so a long conversation does not hold dozens of live pages.
 */
export function ArtifactFrame({
  kind,
  code,
  reloadKey,
  fixedHeight,
}: {
  kind: ArtifactKind;
  code: string;
  reloadKey: number;
  /** Full-size view: fill this height instead of measuring the page. */
  fixedHeight?: string;
}) {
  const id = useId();
  const box = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const [near, setNear] = useState(Boolean(fixedHeight));
  const [height, setHeight] = useState(ARTIFACT_DEFAULT_HEIGHT);

  useEffect(() => {
    if (fixedHeight) return;
    const el = box.current;
    if (!el || typeof IntersectionObserver === "undefined") {
      setNear(true);
      return;
    }
    const io = new IntersectionObserver((es) => es.forEach((e) => setNear(e.isIntersecting)), {
      rootMargin: "800px 0px",
    });
    io.observe(el);
    return () => io.disconnect();
  }, [fixedHeight]);

  useEffect(() => {
    if (fixedHeight) return;
    const on = (e: MessageEvent) => {
      const h = parseHeightMessage(e, frame.current?.contentWindow, id);
      if (h !== null) setHeight(h);
    };
    window.addEventListener("message", on);
    return () => window.removeEventListener("message", on);
  }, [id, fixedHeight]);

  const src = useMemo(() => (near ? artifactSrc(kind, code, id) : ""), [near, kind, code, id]);

  return (
    <div ref={box} style={{ height: fixedHeight ?? height }} className="bg-white">
      {near && (
        <iframe
          key={reloadKey}
          ref={frame}
          title="Preview"
          sandbox={ARTIFACT_SANDBOX}
          referrerPolicy="no-referrer"
          loading="lazy"
          src={src}
          className="block h-full w-full border-0"
        />
      )}
    </div>
  );
}

export function ArtifactModal({
  kind,
  code,
  open,
  onClose,
}: {
  kind: ArtifactKind;
  code: string;
  open: boolean;
  onClose: () => void;
}) {
  return (
    <D.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <D.Portal>
        <D.Overlay className="anim-fade fixed inset-0 z-[90] bg-black/70" />
        <D.Content
          aria-describedby={undefined}
          className="anim-fade fixed inset-8 z-[90] flex flex-col overflow-hidden rounded-xl border border-line bg-code focus:outline-none max-sm:inset-2"
        >
          <div className="flex h-9 shrink-0 items-center justify-between border-b border-line/70 bg-panel/50 pl-3.5 pr-1.5">
            <D.Title className="text-caption text-steel">Preview</D.Title>
            <D.Close className={BTN} aria-label="Close">
              <X className="h-3.5 w-3.5" />
            </D.Close>
          </div>
          <div className="min-h-0 flex-1">
            <ArtifactFrame kind={kind} code={code} reloadKey={0} fixedHeight="100%" />
          </div>
        </D.Content>
      </D.Portal>
    </D.Root>
  );
}
