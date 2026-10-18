import { useLayoutEffect, useRef, type CSSProperties, type ReactNode } from "react";
import type { OriginRect } from "../router";
import styles from "./Spread.module.css";

/** The transform that makes the spread start at the clicked element's rect (A/1's push-in). */
export function pushFrom(origin: OriginRect, target: OriginRect): string {
  const sx = Math.max(0.15, origin.width / Math.max(1, target.width));
  const sy = Math.max(0.15, origin.height / Math.max(1, target.height));
  const tx = origin.left + origin.width / 2 - (target.left + target.width / 2);
  const ty = origin.top + origin.height / 2 - (target.top + target.height / 2);
  return `translate(${tx.toFixed(1)}px, ${ty.toFixed(1)}px) scale(${sx.toFixed(3)}, ${sy.toFixed(3)})`;
}

export function Spread(props: {
  open: boolean;
  /** Changes when the nested page changes, so scroll resets once per page, not per render. */
  pageKey: string;
  /** Read once when the spread opens: where the operator clicked. */
  takeOrigin: () => OriginRect | null;
  narrow: boolean;
  busOffset: string;
  label: string;
  children: ReactNode;
}) {
  const ref = useRef<HTMLElement>(null);
  const { open, pageKey, takeOrigin } = props;

  // The push-in is a one-shot DOM animation, written to the element, not to React state.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !open) return;
    const origin = takeOrigin();
    if (origin) {
      el.style.setProperty("--push-from", pushFrom(origin, el.getBoundingClientRect()));
      el.dataset["push"] = "true";
    }
    el.focus({ preventScroll: true });
  }, [open, takeOrigin]);

  useLayoutEffect(() => {
    if (ref.current) ref.current.scrollTop = 0;
  }, [pageKey]);

  const style = { "--bus-offset": props.busOffset } as CSSProperties;
  return (
    <main
      ref={ref}
      className={styles.spread}
      data-open={open}
      data-narrow={props.narrow}
      data-role="spread"
      tabIndex={-1}
      aria-label={props.label}
      aria-hidden={!open}
      style={style}
      onAnimationEnd={(e) => {
        if (e.target === e.currentTarget) delete e.currentTarget.dataset["push"];
      }}
    >
      <div className={styles.inner}>{props.children}</div>
    </main>
  );
}
