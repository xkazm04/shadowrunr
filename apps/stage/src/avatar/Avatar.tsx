// `<Avatar>`: Malakor's face in the stage's avatar slot.
//
// A box of `size` CSS pixels holding one canvas. Everything that draws is in the chosen variant's
// runtime (`variant`: `glow` is `./runtime`, see variants.ts), imported with `import()` when the face
// mounts, so the stage pays nothing for it until then and pays after its first paint. `look` says
// where he looks; its change events reach the runtime (which wakes on them and reads the look on its
// own frames), and a new `look` object is one more change, never a restart. Until the form's layers are decoded the box is empty and transparent; when
// WebGL2 is not there, or the form cannot load, it stays empty and the runtime says why once in the
// console, so the slot keeps its silhouette instead. The face never re-renders React: signals go
// from `source` straight to the runtime. `data-face` on the box reads loading | ready |
// unavailable, `data-face-loop` running | still, `data-form` rig | placeholder. `onFace` hears
// `silhouette` on mount and `rig` once WebGL draws the daemon's real layer rig; the procedural
// placeholder stays `silhouette` (the report to the daemon means "Malakor's real face").

import { useEffect, useRef } from "react";

import { faceStateOf, type FaceState } from "../voice/report";
import type { FaceSource } from "./types";
import { DEFAULT_FACE, loadVariant, type FaceVariant, type Look, type LookSource } from "./variants";

const AHEAD: Look = Object.freeze({ x: 0, y: 0 });

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
  /** Which renderer draws the face (`?avatar=`; default `cut`). */
  variant?: FaceVariant;
  /** Where he looks: a change wakes the face, which reads it on its own frames. */
  look?: LookSource;
  /** The face's accessible name: the stream's host name (default "Host"). */
  label?: string;
}

type Handle = { resize(size: number): void; dispose(): void };

export function Avatar({ rigBase, size, source, className, onFace, variant = DEFAULT_FACE, look, label = "Host" }: AvatarProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const handleRef = useRef<Handle | null>(null);
  const sizeRef = useRef(size);
  const onFaceRef = useRef(onFace);
  onFaceRef.current = onFace;
  const lookRef = useRef(look);
  lookRef.current = look;
  // the face's look listeners outlive any one look object: they are told by whichever is current
  const lookListeners = useRef(new Set<(l: Look) => void>()).current;
  const firstLook = useRef(true);
  useEffect(() => {
    const tell = (l: Look) => {
      for (const fn of [...lookListeners]) fn(l);
    };
    if (firstLook.current) firstLook.current = false;
    else tell(look?.get() ?? AHEAD);
    return look?.subscribe?.(tell);
  }, [look, lookListeners]);

  useEffect(() => {
    const root = rootRef.current;
    const canvas = canvasRef.current;
    if (!root || !canvas) return;
    let cancelled = false;
    const drawn = (form: "rig" | "placeholder" | null) => {
      if (!cancelled) onFaceRef.current?.(faceStateOf(form));
    };
    drawn(null);
    // the look is read through the ref, so a new look object never restarts the face
    const lookSource: LookSource = {
      get: () => lookRef.current?.get() ?? AHEAD,
      subscribe(fn) {
        lookListeners.add(fn);
        return () => void lookListeners.delete(fn);
      },
    };
    loadVariant(variant).then(
      (startFace) => {
        if (cancelled) return;
        handleRef.current = startFace({ root, canvas, rigBase, source, size: sizeRef.current, onDrawn: drawn, look: lookSource });
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
  }, [rigBase, source, variant, lookListeners]);

  useEffect(() => {
    sizeRef.current = size;
    handleRef.current?.resize(size);
  }, [size]);

  return (
    <div
      ref={rootRef}
      className={className}
      role="img"
      aria-label={label}
      data-face="loading"
      data-variant={variant}
      style={{ width: size, height: size }}
    >
      <canvas ref={canvasRef} aria-hidden="true" style={{ visibility: "hidden" }} />
    </div>
  );
}
