// A canvas that redraws only when its draw function (memoised on the model) or its size
// changes: no animation frame loop. `data-draws` counts the draws so tests can prove it.
import { useEffect, useRef } from "react";
import type { Ctx, Size } from "./draw";

function measure(el: HTMLCanvasElement): Size {
  return { w: el.clientWidth || 300, h: el.clientHeight || 150 };
}

export function Plot(props: { draw: (c: Ctx, size: Size) => void; className?: string | undefined; role: string; label: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const state = useRef<{ draw: (c: Ctx, size: Size) => void; size: Size | null; draws: number } | null>(null);

  const { draw } = props;
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const s = state.current ?? { draw, size: null, draws: 0 };
    state.current = s;
    s.draw = draw;
    paint(el, s);
  }, [draw]);

  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      const s = state.current;
      if (!s) return;
      const next = measure(el);
      if (s.size && s.size.w === next.w && s.size.h === next.h) return;
      paint(el, s);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  return <canvas ref={ref} className={props.className} data-role={props.role} role="img" aria-label={props.label} />;
}

function paint(el: HTMLCanvasElement, s: { draw: (c: Ctx, size: Size) => void; size: Size | null; draws: number }) {
  const size = measure(el);
  s.size = size;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const pw = Math.round(size.w * dpr);
  const ph = Math.round(size.h * dpr);
  if (el.width !== pw || el.height !== ph) {
    el.width = pw;
    el.height = ph;
  }
  const c = el.getContext("2d");
  if (!c) return;
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  s.draw(c, size);
  s.draws += 1;
  el.dataset["draws"] = String(s.draws);
}
