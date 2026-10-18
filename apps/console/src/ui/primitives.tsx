import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { HealthState } from "../contract";
import { DAEMON_START_COMMAND, RpcError, isOffline } from "../api/rpc";

export function cx(...xs: (string | false | null | undefined)[]): string {
  return xs.filter(Boolean).join(" ");
}

export function Button(props: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "default" | "primary" | "ghost" | "danger";
  disabled?: boolean;
  title?: string;
  type?: "button" | "submit";
  kbd?: string;
}) {
  return (
    <button
      type={props.type ?? "button"}
      className={cx("btn", props.variant && props.variant !== "default" && `btn-${props.variant}`)}
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.title}
    >
      {props.children}
      {props.kbd && <kbd>{props.kbd}</kbd>}
    </button>
  );
}

export function Dot({ state }: { state: HealthState | "none" }) {
  return <span className={cx("dot", `dot-${state}`)} aria-label={state} />;
}

export function Chip(props: {
  children: ReactNode;
  tone?: "neutral" | "ok" | "warn" | "bad" | "accent" | "prevent" | "try";
  onClick?: () => void;
  title?: string;
}) {
  const cls = cx("chip", `chip-${props.tone ?? "neutral"}`, props.onClick && "chip-click");
  return props.onClick ? (
    <button type="button" className={cls} onClick={props.onClick} title={props.title}>
      {props.children}
    </button>
  ) : (
    <span className={cls} title={props.title}>
      {props.children}
    </span>
  );
}

export function Toolbar(props: { title: string; subtitle?: ReactNode; children?: ReactNode }) {
  return (
    <header className="toolbar">
      <div className="toolbar-title">
        <h1>{props.title}</h1>
        {props.subtitle && <span className="toolbar-sub">{props.subtitle}</span>}
      </div>
      <div className="toolbar-actions">{props.children}</div>
    </header>
  );
}

export function SectionHeader(props: { children: ReactNode; right?: ReactNode }) {
  return (
    <div className="section-h">
      <span>{props.children}</span>
      {props.right}
    </div>
  );
}

export function Empty(props: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-title">{props.title}</div>
      {props.children && <div className="empty-body">{props.children}</div>}
    </div>
  );
}

export function Loading({ label = "Loading" }: { label?: string }) {
  return <div className="loading">{label}…</div>;
}

function CopyButton({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <Button
      variant="ghost"
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setDone(true);
          setTimeout(() => setDone(false), 1200);
        });
      }}
    >
      {done ? "Copied" : "Copy"}
    </Button>
  );
}

/** The daemon-offline empty state: what to run, retry, or review with fixtures. */
export function OfflineState({ onRetry }: { onRetry?: () => void }) {
  const mockHref = `${window.location.pathname}?mock=1${window.location.hash}`;
  return (
    <div className="empty offline">
      <div className="empty-title">The daemon is not running</div>
      <div className="empty-body">Start it from the repository root:</div>
      <div className="cmd">
        <code>{DAEMON_START_COMMAND}</code>
        <CopyButton text={DAEMON_START_COMMAND} />
      </div>
      <div className="empty-actions">
        {onRetry && (
          <Button variant="primary" onClick={onRetry}>
            Retry
          </Button>
        )}
        <a className="btn btn-ghost" href={mockHref}>
          Review with fixtures
        </a>
      </div>
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  if (isOffline(error)) return <OfflineState onRetry={onRetry} />;
  const e = error instanceof RpcError ? error : undefined;
  return (
    <div className="empty error">
      <div className="empty-title">
        {e ? `${e.method} failed` : "Something went wrong"}
        {e?.code && <Chip tone="bad">{e.code}</Chip>}
      </div>
      <div className="empty-body">{error instanceof Error ? error.message : String(error)}</div>
      {onRetry && (
        <div className="empty-actions">
          <Button onClick={onRetry}>Retry</Button>
        </div>
      )}
    </div>
  );
}

/** Inline error line for actions (not a whole-pane replacement). */
export function ErrorLine({ error }: { error: unknown }) {
  if (!error) return null;
  const msg = isOffline(error)
    ? `Daemon offline. Run: ${DAEMON_START_COMMAND}`
    : error instanceof Error
      ? error.message
      : String(error);
  return <div className="error-line">{msg}</div>;
}

/**
 * Keyboard list navigation: Up/Down/Home/End move the active row, Enter/Space activate it.
 * The container is one tab stop (roving focus via aria-activedescendant).
 */
export function useListNav(count: number, opts: { onActivate?: (i: number) => void; onMove?: (i: number) => void; initial?: number } = {}) {
  const [active, setActive] = useState(opts.initial ?? -1);
  const ref = useRef<HTMLDivElement>(null);
  const clamp = (i: number) => Math.max(0, Math.min(count - 1, i));

  useEffect(() => {
    if (active >= count) setActive(count - 1);
  }, [count, active]);

  useLayoutEffect(() => {
    if (active < 0) return;
    const el = ref.current?.querySelector<HTMLElement>(`[data-row="${active}"]`);
    el?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  const move = (i: number) => {
    if (count === 0) return;
    const n = clamp(i);
    setActive(n);
    opts.onMove?.(n);
  };

  const onKeyDown = (e: KeyboardEvent) => {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(active < 0 ? 0 : active + 1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(active < 0 ? 0 : active - 1);
        break;
      case "Home":
        e.preventDefault();
        move(0);
        break;
      case "End":
        e.preventDefault();
        move(count - 1);
        break;
      case "Enter":
      case " ":
        if (active >= 0) {
          e.preventDefault();
          opts.onActivate?.(active);
        }
        break;
    }
  };

  return {
    active,
    setActive,
    containerProps: { ref, tabIndex: 0, onKeyDown, role: "listbox" as const },
    rowProps: (i: number) => ({
      "data-row": i,
      role: "option" as const,
      "aria-selected": i === active,
      onMouseDown: () => setActive(i),
    }),
  };
}

/** A macOS-style sheet: slides from the top of the window, Esc or the backdrop closes it. */
export function Sheet(props: { title: string; onClose: () => void; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(props.onClose);
  closeRef.current = props.onClose;
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    ref.current?.focus();
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        closeRef.current();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      prev?.focus?.();
    };
  }, []);
  return (
    <div className="sheet-backdrop" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
      <div className={cx("sheet", props.wide && "sheet-wide")} role="dialog" aria-modal="true" aria-label={props.title} tabIndex={-1} ref={ref}>
        <div className="sheet-h">
          <span>{props.title}</span>
          <button type="button" className="icon-btn" onClick={props.onClose} aria-label="Close">
            ×
          </button>
        </div>
        <div className="sheet-body">{props.children}</div>
        {props.footer && <div className="sheet-f">{props.footer}</div>}
      </div>
    </div>
  );
}

/** Anchored popover; closes on outside click or Esc. */
export function Popover(props: { trigger: (open: () => void) => ReactNode; children: (close: () => void) => ReactNode }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: globalThis.KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);
  return (
    <span className="popover-anchor" ref={ref}>
      {props.trigger(() => setOpen((o) => !o))}
      {open && <div className="popover">{props.children(() => setOpen(false))}</div>}
    </span>
  );
}

export function Segmented<T extends string>(props: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={props.label}>
      {props.options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === props.value}
          className={cx(o.value === props.value && "on")}
          onClick={() => props.onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** The right-hand inspector pane. */
export function Inspector(props: {
  title: ReactNode;
  onClose?: () => void;
  actions?: ReactNode;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <aside className={cx("inspector", props.wide && "inspector-wide")} aria-label="Inspector">
      <div className="inspector-h">
        <div className="inspector-title">{props.title}</div>
        <div className="inspector-actions">
          {props.actions}
          {props.onClose && (
            <button type="button" className="icon-btn" onClick={props.onClose} aria-label="Close inspector" title="Close (Esc)">
              ×
            </button>
          )}
        </div>
      </div>
      <div className="inspector-body">{props.children}</div>
    </aside>
  );
}

export function KV({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="kv">
      {rows.map(([k, v]) => (
        <div key={k}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}
