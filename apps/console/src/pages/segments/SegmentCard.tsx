// One segment's table (l2b.js `.seg-full`): the state chain, allowed ops, screens, commands,
// hand-overs, the clock with SEG-01's cold open and handover ruler, the per-state rules, the
// arc, the transitions and the rules prompt. A descoped table prints its design notes: it has
// no states to run. Every rule the runtime reads is printed: the daemon test
// `segment_tables_carry_every_declared_rule` holds the payload to the TOML.
import type { ArcInfo, ClockInfo, ColdOpenInfo, HandoverInfo, SegmentTableInfo, StateRulesInfo } from "@shadowrunr/contract";
import type { CSSProperties } from "react";
import { pageRoute, RouteLink } from "../../router";
import { Chip } from "../../ui/Chip";
import styles from "./Layer2.module.css";
import { fmtSecs, handoverMarks, humanState, STATUS_TONE } from "./model";

export function SegmentCard({ seg, focused }: { seg: SegmentTableInfo; focused: boolean }) {
  const off = seg.status === "descoped";
  return (
    <article className={`${styles.segFull} ${off ? styles.off : ""} ${focused ? styles.flash : ""}`} data-role="segments-card" data-segment={seg.id}>
      <header>
        <span className={styles.sid}>{seg.id}</span>
        <h2>{seg.title}</h2>
        <Chip tone={STATUS_TONE[seg.status]}>{off ? "descoped · never started" : seg.status}</Chip>
      </header>
      {off ? (
        <>
          <p className={styles.prose}>{seg.design_notes ?? "Descoped: the runtime refuses to start this segment."}</p>
          <p className={styles.note}>Descoped is not retired: the segment never ran, so it holds no lifecycle place.</p>
        </>
      ) : (
        <Runnable seg={seg} />
      )}
    </article>
  );
}

function Runnable({ seg }: { seg: SegmentTableInfo }) {
  return (
    <>
      <div className={styles.chain} data-role="segments-states">
        {seg.states.map((x, i) => (
          <span key={x} className={styles.chainStep}>
            {i > 0 && <em aria-hidden="true" />}
            <span className={x === seg.initial ? styles.init : undefined} title={x === seg.initial ? "Initial state" : undefined}>
              {humanState(x)}
            </span>
          </span>
        ))}
      </div>
      <div className={styles.cols2}>
        <div>
          <h3>
            Allowed ops <small>{seg.allowed_ops.length}</small>
          </h3>
          <div className={styles.chips}>
            {seg.allowed_ops.map((o) => (
              <code key={o}>{o}</code>
            ))}
          </div>
          {seg.screens && seg.screens.length > 0 && (
            <>
              <h3>Screens</h3>
              <div className={styles.chips} data-role="segments-screens">
                {seg.screens.map((x, i) => (
                  <code key={x} title={i === 0 ? "Default at segment start" : undefined}>
                    {x}
                  </code>
                ))}
              </div>
            </>
          )}
          <h3>Chat commands</h3>
          <div className={styles.chips}>
            {seg.commands.map((c) => (
              <code key={c}>!{c}</code>
            ))}
          </div>
          <h3>Hands over to</h3>
          <div className={styles.chips}>
            {seg.switch_to.length === 0 ? (
              <span className={styles.dim}>none</span>
            ) : (
              seg.switch_to.map((id) => (
                <RouteLink key={id} to={pageRoute("segments", "tables", id)} className={styles.ref}>
                  {id}
                </RouteLink>
              ))
            )}
          </div>
        </div>
        <div>
          <h3>
            Clock <em className={styles.asm}>assumptions</em>
          </h3>
          <Clock clock={seg.clock} research={seg.allowed_ops.includes("dispatch_research")} />
        </div>
      </div>
      {seg.state_rules && Object.keys(seg.state_rules).length > 0 && <StateRules rules={seg.state_rules} />}
      {seg.arc && <Arc arc={seg.arc} />}
      <h3>
        Transitions <small>{seg.transitions.length} rows</small>
      </h3>
      {seg.transitions.length > 0 && (
        <div className={styles.tblwrap}>
          <table className={`${styles.tbl} ${styles.dense}`} data-role="segments-transitions">
            <thead>
              <tr>
                <th>From</th>
                <th>On</th>
                <th>Guard</th>
                <th>To</th>
              </tr>
            </thead>
            <tbody>
              {seg.transitions.map((t, i) => (
                <tr key={i}>
                  <td>{t.from === "*" ? "any state" : humanState(t.from)}</td>
                  <td>
                    <code>{t.on}</code>
                  </td>
                  <td>{t.guard ?? ""}</td>
                  <td>{humanState(t.to)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <details className={styles.details}>
        <summary>The segment’s rules prompt</summary>
        <pre className={styles.pre}>{seg.rules_prompt.trim()}</pre>
      </details>
    </>
  );
}

function Clock({ clock, research }: { clock: ClockInfo; research: boolean }) {
  return (
    <>
      <dl className={styles.kv}>
        <dt>foil vote</dt>
        <dd>{fmtSecs(clock.foil_vote_secs)}</dd>
        <dt>poll length</dt>
        <dd>
          {fmtSecs(clock.poll_min_secs)} to {fmtSecs(clock.poll_max_secs)}
        </dd>
        <dt>poll options</dt>
        <dd>up to {clock.max_poll_options}</dd>
        {research && clock.max_searches_per_hex !== undefined && (
          <>
            <dt>searches per hex</dt>
            <dd>
              {clock.max_searches_per_hex}, then a {fmtSecs(clock.plebiscite_secs ?? 0)} Plebiscite
            </dd>
          </>
        )}
      </dl>
      {clock.cold_open && <ColdOpen c={clock.cold_open} />}
      {clock.handover ? <Handover h={clock.handover} cold={clock.cold_open !== undefined} /> : <p className={styles.note}>No handover clock: this segment runs until the brain switches.</p>}
    </>
  );
}

/** The cold open: the room ends it inside a floor and a ceiling. */
function ColdOpen({ c }: { c: ColdOpenInfo }) {
  return (
    <p className={styles.note} data-role="segments-cold-open">
      Cold open: moves to {humanState(c.to)} once {c.active_chatters} chatters (each wrote in the last {fmtSecs(c.window_s)}) hold for {fmtSecs(c.sustain_s)}, never before {fmtSecs(c.min_s)}; at {fmtSecs(c.max_s)} it ends regardless.
    </p>
  );
}

/** The handover clock as a ruler: sustain, then the max-elapsed cut-off, then the grace. */
function Handover({ h, cold }: { h: HandoverInfo; cold: boolean }) {
  const m = handoverMarks(h);
  /** A CSS length along the ruler: `n` is already a percentage. */
  const at = (n: number) => `${n.toFixed(2)}%`;
  return (
    <div className={styles.handover} data-role="segments-handover">
      <p className={styles.hoLede}>
        Hands over to{" "}
        <RouteLink to={pageRoute("segments", "tables", h.to)} className={styles.ref}>
          {h.to}
        </RouteLink>{" "}
        when at least {h.ccv_min} viewers hold for {fmtSecs(h.sustain_s)}{cold ? " after the cold open" : ""}, or at {fmtSecs(h.max_elapsed_s)}, whichever comes first. Either way the brain then has {fmtSecs(h.grace_s)} to switch before the clock switches itself.
      </p>
      <div className={styles.ruler} role="img" aria-label={`Handover ruler: sustain ${fmtSecs(h.sustain_s)}, cut-off ${fmtSecs(h.max_elapsed_s)}, grace ${fmtSecs(h.grace_s)}`}>
        <i className={styles.rSustain} style={{ width: at(m.sustainPct) } as CSSProperties} />
        <i className={styles.rGrace} style={{ left: at(m.maxPct), width: at(100 - m.maxPct) } as CSSProperties} />
        <b className={styles.rMark} style={{ left: at(m.maxPct) } as CSSProperties} />
      </div>
      <div className={styles.rLegend}>
        <span>0:00</span>
        <span>
          ccv ≥ {h.ccv_min} for {fmtSecs(h.sustain_s)}
        </span>
        <span>cut-off {fmtSecs(h.max_elapsed_s)}</span>
        <span>+{fmtSecs(h.grace_s)} grace at the latest, then the clock switches itself</span>
      </div>
    </div>
  );
}

/** What each state narrows; "as the segment" is the segment's own list. */
function StateRules({ rules }: { rules: Readonly<Record<string, StateRulesInfo | undefined>> }) {
  const list = (xs: readonly string[] | undefined) => (xs === undefined ? <span className={styles.dim}>as the segment</span> : xs.map((x) => <code key={x}>{x}</code>));
  return (
    <>
      <h3>
        Per state <small>{Object.keys(rules).length}</small>
      </h3>
      <div className={styles.tblwrap}>
        <table className={`${styles.tbl} ${styles.dense}`} data-role="segments-state-rules">
          <thead>
            <tr>
              <th>State</th>
              <th>Ops</th>
              <th>Screens</th>
              <th>Rules line</th>
            </tr>
          </thead>
          <tbody>
            {Object.entries(rules).flatMap(([state, r]) => (r === undefined ? [] : [
              <tr key={state}>
                <td>{humanState(state)}</td>
                <td>{list(r.allowed_ops)}</td>
                <td>{list(r.screens)}</td>
                <td>{r.prompt ?? ""}</td>
              </tr>,
            ]))}
          </tbody>
        </table>
      </div>
    </>
  );
}

/** The multi-act arc of a long stream. */
function Arc({ arc }: { arc: ArcInfo }) {
  return (
    <div data-role="segments-arc">
      <h3>
        Arc <small>{arc.acts.length} acts</small>
      </h3>
      <p className={styles.note}>
        Runs when the segment is planned for at least {fmtSecs(arc.min_planned_s)}: {arc.acts.join(", then ")}. Sign-off waits for the last {fmtSecs(arc.seal_s)}.
      </p>
      <dl className={styles.kv}>
        <dt>reveal pacing</dt>
        <dd>{arc.reveal_pacing ? "one slot per staged hex" : "off"}</dd>
        <dt>reveal gap</dt>
        <dd>{arc.reveal_gap_pct > 0 ? `${arc.reveal_gap_pct}% of a slot` : "off"}</dd>
        <dt>reveal needs work</dt>
        <dd>{arc.reveal_needs_work ? "a search or a closed vote first" : "no"}</dd>
        <dt>recap cooldown</dt>
        <dd>{arc.recap_cooldown_s > 0 ? `${fmtSecs(arc.recap_cooldown_s)} (direction only)` : "off"}</dd>
      </dl>
    </div>
  );
}
