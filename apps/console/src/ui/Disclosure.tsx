import { useState, type KeyboardEvent, type ReactNode } from "react";
import { cx } from "./primitives";

/** Move focus between `[data-xrow]` elements inside the nearest `[data-xrows]` container. */
function moveFocus(e: KeyboardEvent<HTMLElement>, delta: number) {
  const root = e.currentTarget.closest("[data-xrows]");
  if (!root) return;
  const rows = [...root.querySelectorAll<HTMLElement>("[data-xrow]")];
  const i = rows.indexOf(e.currentTarget);
  const next = rows[Math.max(0, Math.min(rows.length - 1, i + delta))];
  next?.focus();
}

/** An indicative row that expands into detail (Enter/Space or click; Up/Down move). */
export function DisclosureRow(props: { summary: ReactNode; children?: ReactNode; className?: string; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(props.defaultOpen ?? false);
  const expandable = props.children !== undefined && props.children !== null && props.children !== false;
  return (
    <div className={cx("xrow-wrap", open && "open")}>
      <div
        className={cx("xrow", props.className, expandable && "expandable")}
        data-xrow=""
        tabIndex={0}
        role={expandable ? "button" : undefined}
        aria-expanded={expandable ? open : undefined}
        onClick={() => expandable && setOpen((o) => !o)}
        onKeyDown={(e) => {
          if ((e.key === "Enter" || e.key === " ") && expandable) {
            e.preventDefault();
            setOpen((o) => !o);
          } else if (e.key === "ArrowDown") {
            e.preventDefault();
            moveFocus(e, 1);
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            moveFocus(e, -1);
          } else if (e.key === "ArrowRight" && expandable) {
            setOpen(true);
          } else if (e.key === "ArrowLeft" && expandable) {
            setOpen(false);
          }
        }}
      >
        <span className="caret">{expandable ? (open ? "▾" : "▸") : ""}</span>
        {props.summary}
      </div>
      {open && expandable && <div className="xrow-detail">{props.children}</div>}
    </div>
  );
}
