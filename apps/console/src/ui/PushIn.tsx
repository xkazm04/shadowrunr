import { useLayoutEffect, useRef, type ReactNode } from "react";
import { pushFrom } from "./push";
import styles from "./PushIn.module.css";

/**
 * An in-page list-to-detail move in the shell's language: when `trigger` changes (not on the
 * first render), the detail pushes in from the element `from` returns, the row that was picked,
 * as the spread pushes in from a lit element. Without an origin, or with reduced motion, the
 * detail simply appears.
 */
export function PushIn(props: { readonly trigger: string | undefined; readonly from: () => Element | null; readonly children: ReactNode; readonly "data-role"?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  const from = useRef(props.from);
  const { trigger } = props;

  useLayoutEffect(() => {
    from.current = props.from;
  });

  useLayoutEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    const el = ref.current;
    const origin = from.current();
    if (!el || !origin || trigger === undefined || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;
    el.style.setProperty("--push-from", pushFrom(origin.getBoundingClientRect(), el.getBoundingClientRect()));
    delete el.dataset["push"];
    void el.offsetWidth;
    el.dataset["push"] = "true";
  }, [trigger]);

  return (
    // data-push stays after the move: dropping it would hand the element back to the spread's
    // entrance rise, which would then replay. The next pick restarts the push by re-setting it.
    <div ref={ref} className={styles.push} data-role={props["data-role"]}>
      {props.children}
    </div>
  );
}
