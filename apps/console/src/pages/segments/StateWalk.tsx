// Walking a segment's state machine: the state chain is a radio group (arrow keys move), and
// the panel under it prints what is open in the picked state (ops, screens and commands as
// `state_rules` narrow them, the rules line) and every exit and entry by trigger and guard, each
// target a link to segments/tables/<SEG>/<state>. Everything comes from the daemon's table
// (`segments.tables`); stateView only joins the rows, it decides nothing.
import type { SegmentTableInfo } from "@shadowrunr/contract";
import { useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { encodeRoute, pageRoute, useRouter } from "../../router";
import styles from "./Layer2.module.css";
import { humanState, reachable, stateView, type StateEdge } from "./model";

const STEP: Readonly<Record<string, number>> = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };

export function StateWalk({ seg, state, focused }: { seg: SegmentTableInfo; state: string | undefined; focused: boolean }) {
  const router = useRouter();
  const chain = useRef<HTMLDivElement>(null);
  // The route's state wins whenever it changes; a pick in between is local.
  const [picked, setPicked] = useState({ from: state, value: state ?? seg.initial });
  const selected = picked.from === state ? picked.value : (state ?? seg.initial);
  const stranded = seg.states.filter((s) => !reachable(seg).has(s));
  const view = stateView(seg, selected);

  const walk = (to: string) => {
    setPicked({ from: state, value: to });
    if (focused) router.navigate(pageRoute("segments", "tables", seg.id, to), { replace: true });
  };
  const onKey = (e: KeyboardEvent) => {
    const i = seg.states.indexOf(selected);
    const n = seg.states.length;
    const next = e.key === "Home" ? 0 : e.key === "End" ? n - 1 : STEP[e.key] === undefined ? -1 : (i + (STEP[e.key] ?? 0) + n) % n;
    const to = seg.states[next];
    if (to === undefined) return;
    e.preventDefault();
    walk(to);
    chain.current?.querySelectorAll<HTMLButtonElement>("[role='radio']")[next]?.focus();
  };
  const target = (to: string) => <Walk to={to} seg={seg.id} walk={walk} />;

  return (
    <>
      <div ref={chain} className={styles.chain} role="radiogroup" aria-label={`States of ${seg.id}`} data-role="segments-states" onKeyDown={onKey}>
        {seg.states.map((x) => (
          <button
            key={x}
            type="button"
            role="radio"
            aria-checked={x === selected}
            tabIndex={x === selected ? 0 : -1}
            className={`${styles.step} ${x === seg.initial ? styles.init : ""}`}
            title={x === seg.initial ? "Initial state" : undefined}
            data-unreachable={stranded.includes(x) ? "" : undefined}
            onClick={() => walk(x)}
          >
            {humanState(x)}
          </button>
        ))}
      </div>
      {stranded.length > 0 && <p className={styles.note}>{stranded.map((s) => `${humanState(s)}: unreachable from ${humanState(seg.initial)}`).join("; ")}</p>}
      <div className={styles.cols2} data-role="segments-state-view" data-state={selected}>
        <div>
          <h3>
            Open in {humanState(selected)} <small>{view.ops.length} ops</small>
          </h3>
          <Chips role="segments-state-ops" xs={view.ops} narrowed={view.opsNarrowed} />
          {view.screens.length > 0 && <Chips label="Screens" xs={view.screens} narrowed={view.screensNarrowed} />}
          <Chips label="Chat commands" xs={view.commands.map((c) => `!${c}`)} narrowed={view.commandsNarrowed} />
          {view.prompt && <p className={styles.note}>{view.prompt}</p>}
        </div>
        <div>
          <h3>
            Exits <small>{view.exits.length}</small>
          </h3>
          <ul className={styles.edges}>
            {view.exits.map((e, i) => (
              <li key={i} {...edgeData("segments-state-exit", e)}>
                {trigger(e)} → {target(e.to)}
                {e.to === e.from && <span className={styles.dim}> (stays)</span>}
                {e.any && <span className={styles.dim}> · any state</span>}
              </li>
            ))}
          </ul>
          <h3>
            Entries <small>{view.entries.length}</small>
          </h3>
          <ul className={styles.edges}>
            {view.entries.length === 0 && <li className={styles.dim}>{view.initial ? "none: the segment starts here" : "none"}</li>}
            {view.entries.map((e, i) => (
              <li key={i} {...edgeData("segments-state-entry", e)}>
                {target(e.from)} via {trigger(e)}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </>
  );
}

const edgeData = (role: string, e: StateEdge) => ({
  "data-role": role,
  "data-on": e.on,
  "data-guard": e.guard,
  "data-to": e.to,
  "data-clock": e.clock ? "true" : undefined,
});

function trigger(e: StateEdge): ReactNode {
  if (e.clock) return <span className={styles.dim}>{e.on}</span>;
  return (
    <>
      <code>{e.on}</code>
      {e.guard !== undefined && <span className={styles.dim}> if {e.guard}</span>}
    </>
  );
}

function Chips({ xs, narrowed, label, role }: { xs: readonly string[]; narrowed: boolean; label?: string; role?: string }) {
  return (
    <>
      {label !== undefined && (
        <h3>
          {label} <small>{narrowed ? "narrowed" : "as the segment"}</small>
        </h3>
      )}
      <div className={styles.chips} data-role={role}>
        {xs.length === 0 ? <span className={styles.dim}>none</span> : xs.map((x) => <code key={x}>{x}</code>)}
        {label === undefined && <small>{narrowed ? "narrowed" : "as the segment"}</small>}
      </div>
    </>
  );
}

/** A state as a link: a plain click walks the card there; any other click opens the route. */
function Walk({ to, seg, walk }: { to: string; seg: string; walk: (to: string) => void }) {
  const onClick = (e: MouseEvent) => {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    walk(to);
  };
  return (
    <a href={encodeRoute(pageRoute("segments", "tables", seg, to))} className={styles.ref} onClick={onClick}>
      {humanState(to)}
    </a>
  );
}
