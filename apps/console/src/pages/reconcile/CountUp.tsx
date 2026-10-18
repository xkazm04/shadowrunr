// The winner's count-up numeral (P3.count): ease-out cubic from 0 to the value when the poster
// becomes active. A one-shot animation written to the element, never to React state, so the
// page does not re-render per frame. Reduced motion, or no animation frames, shows the value.
import { useLayoutEffect, useRef } from "react";

const fmt = (v: number) => Math.round(v).toLocaleString("en-US");

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function CountUp(props: { value: number; run: boolean; durationMs?: number; className?: string | undefined }) {
  const ref = useRef<HTMLSpanElement>(null);
  const { value, run } = props;
  const dur = props.durationMs ?? 1300;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (!run || prefersReducedMotion() || typeof window.requestAnimationFrame !== "function") {
      el.textContent = fmt(value);
      return;
    }
    let frame = 0;
    const t0 = performance.now();
    const step = (t: number) => {
      // A frame timestamp may precede t0 (it is the frame start), so clamp at 0 as well.
      const k = Math.max(0, Math.min(1, (t - t0) / dur));
      el.textContent = fmt(value * (1 - Math.pow(1 - k, 3)));
      if (k < 1) frame = window.requestAnimationFrame(step);
    };
    el.textContent = fmt(0);
    frame = window.requestAnimationFrame(step);
    return () => window.cancelAnimationFrame(frame);
  }, [value, run, dur]);

  // No React children: the effect owns the text, so React never touches a node it did not write.
  return <span ref={ref} className={props.className} data-n={value} />;
}
