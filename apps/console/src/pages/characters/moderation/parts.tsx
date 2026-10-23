// Small parts the Moderation tab's panels share: a poll while visible, an inline confirmation,
// and the local field (the console has no shared Field).
import { useEffect, useRef, type ReactNode } from "react";
import { Button } from "../../../ui/Button";
import s from "./Moderation.module.css";

/**
 * Calls `tick` every `ms` while `active` and the document is visible. The moderator pushes no
 * `/events` frames, so its status and log are polled (contract appendix, moderation).
 */
export function usePoll(tick: () => void, ms: number, active = true): void {
  const ref = useRef(tick);
  useEffect(() => {
    ref.current = tick;
  });
  useEffect(() => {
    if (!active) return;
    const t = window.setInterval(() => {
      if (document.visibilityState !== "hidden") ref.current();
    }, ms);
    return () => window.clearInterval(t);
  }, [ms, active]);
}

/** An inline question with a confirming and a cancelling button. */
export function Confirm(props: { text: ReactNode; yes: string; onYes: () => void; onNo: () => void; pending?: boolean; role: string; danger?: boolean }) {
  return (
    <div className={s.confirm} role="alertdialog" aria-label={props.yes} data-role={props.role}>
      <p className={s.confirmText}>{props.text}</p>
      <div className={s.actions}>
        <Button size="compact" variant={props.danger ? "ink" : "primary"} disabled={props.pending} onClick={props.onYes} data-role={`${props.role}-yes`}>
          {props.yes}
        </Button>
        <Button size="compact" disabled={props.pending} onClick={props.onNo} data-role={`${props.role}-no`}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

export function Field(props: { label: string; value: string; onChange: (v: string) => void; role: string; placeholder?: string; numeric?: boolean; mono?: boolean }) {
  return (
    <label className={s.field}>
      <span className={s.label}>{props.label}</span>
      <input
        className={props.mono ? `${s.input} ${s.mono}` : s.input}
        type="text"
        inputMode={props.numeric ? "decimal" : undefined}
        value={props.value}
        placeholder={props.placeholder}
        maxLength={500}
        spellCheck={false}
        onChange={(e) => props.onChange(e.target.value)}
        data-role={props.role}
      />
    </label>
  );
}

/** A panel's heading row: title, then whatever sits right of it. */
export function PanelHead(props: { id: string; title: string; children?: ReactNode }) {
  return (
    <div className={s.panelHead}>
      <h2 className={s.secHead} id={props.id}>
        {props.title}
      </h2>
      {props.children}
    </div>
  );
}
