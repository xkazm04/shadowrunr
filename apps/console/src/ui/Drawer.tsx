// The shared side drawer for detail panes. It stays mounted so that it has an exit: closing
// wipes it out with the content it last showed, and the content unmounts once the wipe is over.
// Focus moves into it on open and returns to the opener on close; Esc closes it.
import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Button } from "./Button";
import styles from "./Drawer.module.css";

/** The exit wipe's length (Drawer.module.css `.drawer`); the content unmounts after it. */
export const DRAWER_EXIT_MS = 600;

export interface DrawerProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly title: ReactNode;
  readonly kicker?: ReactNode;
  /** Head buttons beside Close. */
  readonly actions?: ReactNode;
  /** The page whose palette the drawer prints in; it renders outside the page's own element. */
  readonly page?: string;
  /** Close's accessible name, when "Close" alone would be ambiguous. */
  readonly closeLabel?: string;
  readonly "data-role"?: string;
  readonly children: ReactNode;
}

interface Shown {
  readonly title: ReactNode;
  readonly kicker: ReactNode;
  readonly actions: ReactNode;
  readonly children: ReactNode;
}

const isTyping = (t: EventTarget): boolean =>
  t instanceof HTMLElement && (t.isContentEditable || t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT");

/** The shell root: above the chrome and the bus; the document body outside the shell (tests). */
const host = (): HTMLElement => document.querySelector<HTMLElement>("[data-role='shell']") ?? document.body;

export function Drawer(props: DrawerProps) {
  const { open, onClose } = props;
  const ref = useRef<HTMLElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const titleId = useId();
  // What the drawer shows: the live props while open, the last of them through the exit wipe.
  const live: Shown = { title: props.title, kicker: props.kicker, actions: props.actions, children: props.children };
  const [held, setHeld] = useState<Shown | null>(open ? live : null);
  if (open && (held === null || held.title !== live.title || held.kicker !== live.kicker || held.actions !== live.actions || held.children !== live.children)) setHeld(live);

  // Focus in on open (remembering the opener), back to the opener on close.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open) {
      const active = document.activeElement;
      opener.current = active instanceof HTMLElement && !el.contains(active) ? active : null;
      el.focus({ preventScroll: true });
      return;
    }
    const back = opener.current;
    opener.current = null;
    const active = document.activeElement;
    if (back?.isConnected && (active === null || active === document.body || el.contains(active))) back.focus({ preventScroll: true });
  }, [open]);

  // The content unmounts once the exit wipe is over.
  useEffect(() => {
    if (open || held === null) return;
    const t = window.setTimeout(() => setHeld(null), DRAWER_EXIT_MS);
    return () => window.clearTimeout(t);
  }, [open, held]);

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== "Escape" || !open) return;
    // Handled here, so the shell's keymap does not also go up a level.
    e.preventDefault();
    // Esc in a field leaves the field first, inside the drawer; the next Esc closes it.
    if (isTyping(e.target)) ref.current?.focus({ preventScroll: true });
    else onClose();
  };

  return createPortal(
    <aside
      ref={ref}
      className={styles.drawer}
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      aria-hidden={!open}
      tabIndex={-1}
      data-open={open}
      data-page={props.page}
      data-role={props["data-role"] ?? "drawer"}
      onKeyDown={onKeyDown}
    >
      {held && (
        <>
          <div className={styles.head}>
            <div>
              {held.kicker !== undefined && <div className={styles.kicker}>{held.kicker}</div>}
              <h2 id={titleId} className={styles.title}>
                {held.title}
              </h2>
            </div>
            <div className={styles.actions}>
              {held.actions}
              <Button variant="secondary" size="compact" onClick={onClose} aria-label={props.closeLabel} data-role="drawer-close">
                Close
              </Button>
            </div>
          </div>
          <div className={styles.body}>{held.children}</div>
        </>
      )}
    </aside>,
    host(),
  );
}
