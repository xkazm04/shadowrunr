// A real virtualiser (TanStack Virtual): only the rows in view plus `overscan` are in the DOM,
// row heights are measured as they render, so 100k transcript lines cost a screenful of nodes.
import { observeElementRect, useVirtualizer, type Rect } from "@tanstack/react-virtual";
import { useRef, type ReactNode } from "react";
import { cx } from "./cx";
import { remPx } from "./remPx";
import styles from "./VirtualList.module.css";

export interface VirtualListProps {
  readonly count: number;
  readonly renderRow: (index: number) => ReactNode;
  /** Row height estimate in rem; real heights are measured. */
  readonly estimateRem?: number;
  readonly overscan?: number;
  readonly label: string;
  /** Sizes the viewport (it fills its parent by default). */
  readonly className?: string | undefined;
  /** Viewport size before layout (and in tests, where layout is zero). */
  readonly initialRect?: Rect;
  readonly getKey?: (index: number) => string | number;
}

export const DEFAULT_INITIAL_RECT: Rect = { width: 960, height: 640 };

/** A row's real height, or the estimate while it has no layout (hidden panel, jsdom). */
export function measuredHeight(el: Element, estimatePx: number): number {
  const h = el.getBoundingClientRect().height;
  return h > 0 ? h : estimatePx;
}

export function VirtualList(props: VirtualListProps) {
  const ref = useRef<HTMLDivElement>(null);
  const rowPx = (props.estimateRem ?? 2) * remPx();
  const v = useVirtualizer({
    count: props.count,
    getScrollElement: () => ref.current,
    estimateSize: () => rowPx,
    overscan: props.overscan ?? 8,
    initialRect: props.initialRect ?? DEFAULT_INITIAL_RECT,
    measureElement: (el) => measuredHeight(el, rowPx),
    ...(props.getKey ? { getItemKey: props.getKey } : {}),
    // Keep the initial rect until layout reports a real size (jsdom and hidden panels report 0).
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) => {
        if (rect.height > 0) cb(rect);
      }),
  });
  return (
    <div ref={ref} className={cx(styles.viewport, props.className)} tabIndex={0} role="list" aria-label={props.label}>
      <div className={styles.sizer} style={{ height: v.getTotalSize() }}>
        {v.getVirtualItems().map((item) => (
          <div
            key={item.key}
            role="listitem"
            data-index={item.index}
            ref={v.measureElement}
            className={styles.row}
            style={{ transform: `translateY(${item.start}px)` }}
          >
            {props.renderRow(item.index)}
          </div>
        ))}
      </div>
    </div>
  );
}
