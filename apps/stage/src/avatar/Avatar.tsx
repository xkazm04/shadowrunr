// `<Avatar>`: Malakor's face in the stage's avatar slot.
//
// A box of `size` CSS pixels holding one canvas. Everything that draws is in `./runtime`, imported
// with `import()` when the face mounts, so the stage pays nothing for it until then and pays after
// its first paint. Until the form's layers are decoded the box is empty and transparent; when
// WebGL2 is not there, or the form cannot load, it stays empty and the runtime says why once in the
// console, so the slot keeps its silhouette instead. The face never re-renders React: signals go
// from `source` straight to the runtime. `data-face` on the box reads loading | ready |
// unavailable, `data-face-loop` running | still, `data-form` rig | placeholder. `onFace` hears
// `silhouette` on mount and `rig` once WebGL draws the daemon's real layer rig; the procedural
// placeholder stays `silhouette` (the report to the daemon means "Malakor's real face").

import { useEffect, useRef } from "react";

import { faceStateOf, type FaceState } from "../voice/report";
import type { FaceSource } from "./types";

export interface AvatarProps {
  /** The daemon's `/avatar/` base URL (with a trailing slash), or `null` to draw the placeholder. */
  rigBase: string | null;
  /** Square box the face is fitted into, in CSS pixels. */
  size: number;
  /** Where the face signal comes from. */
  source: FaceSource;
  className?: string;
  /** Hears which face the slot shows: on mount, and on every change. */
  onFace?: (state: FaceState) => void;
}

type Handle = { resize(size: number): void; dispose(): void };

export function Avatar({ rigBase, size, source, className, onFace }: AvatarProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const handleRef = useRef<Handle | null>(null);
  const sizeRef = useRef(size);
  const onFaceRef = useRef(onFace);
  onFaceRef.current = onFace;

  useEffect(() => {
    const root = rootRef.current;
    const canvas = canvasRef.current;
    if (!root || !canvas) return;
    let cancelled = false;
    const drawn = (form: "rig" | "placeholder" | null) => {
      if (!cancelled) onFaceRef.current?.(faceStateOf(form));
    };
    drawn(null);
    import("./runtime").then(
      ({ startFace }) => {
        if (cancelled) return;
        handleRef.current = startFace({ root, canvas, rigBase, source, size: sizeRef.current, onDrawn: drawn });
      },
      (error: unknown) => {
        drawn(null);
        root.setAttribute("data-face", "unavailable");
        console.warn(`[stage] Malakor's face is not drawn here: the runtime did not load (${String(error)}).`);
      },
    );
    return () => {
      cancelled = true;
      handleRef.current?.dispose();
      handleRef.current = null;
    };
  }, [rigBase, source]);

  useEffect(() => {
    sizeRef.current = size;
    handleRef.current?.resize(size);
  }, [size]);

  return (
    <div
      ref={rootRef}
      className={className}
      role="img"
      aria-label="Arch-Lich Malakor"
      data-face="loading"
      style={{ width: size, height: size }}
    >
      <canvas ref={canvasRef} aria-hidden="true" style={{ visibility: "hidden" }} />
    </div>
  );
}
