// One segment's table (l2b.js `.seg-full`): the state chain, allowed ops, commands, hand-overs,
// the clock with SEG-01's handover ruler, the transitions and the rules prompt. A descoped
// table prints its design notes: it has no states to run.
import type { ClockInfo, HandoverInfo, SegmentTableInfo } from "@shadowrunr/contract";
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
          <Clock clock={seg.clock} />
        </div>
      </div>
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

function Clock({ clock }: { clock: ClockInfo }) {
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
      </dl>
      {clock.handover ? <Handover h={clock.handover} /> : <p className={styles.note}>No handover clock: this segment runs until the brain switches.</p>}
    </>
  );
}

/** The handover clock as a ruler: sustain, then the max-elapsed cut-off, then the grace. */
function Handover({ h }: { h: HandoverInfo }) {
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
        when at least {h.ccv_min} viewers hold for {fmtSecs(h.sustain_s)}, or at {fmtSecs(h.max_elapsed_s)}, whichever comes first.
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
        <span>+{fmtSecs(h.grace_s)} grace, then the clock switches itself</span>
      </div>
    </div>
  );
}
