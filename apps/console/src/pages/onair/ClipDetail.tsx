// `#/onair/clips/<id>`: one candidate (`clips.get`). The cut as a bar against its band, the
// signals with nominal and applied weights (an unmeasured signal named with its reason, never
// shown as zero), the markers, the claim context, the gate checklist, the exact captions, the
// render and the trail. The owner's acts sit at the foot (ClipActions).
import type { ClipCandidate } from "@shadowrunr/contract";
import type { CSSProperties } from "react";
import { useRpc } from "../../data";
import { QueryView } from "../../kit";
import { Chip, ChipRow } from "../../ui/Chip";
import { Head, Section } from "../../ui/Section";
import { ClipActions } from "./ClipActions";
import { GATE_TONE, SIGNALS, STATUS_LABEL, STATUS_TONE, WEIGHTS_LABEL, clock, pct, score, secs, windowBar } from "./clipModel";
import styles from "./Clips.module.css";

export function ClipDetail({ id }: { id: string }) {
  const q = useRpc("clips.get", { id });
  return (
    <div data-role="onair-clip-detail">
      <QueryView query={q}>{(c) => <Detail c={c} />}</QueryView>
    </div>
  );
}

function Detail({ c }: { c: ClipCandidate }) {
  return (
    <>
      <Head
        kicker={`Clip · rank ${c.rank} of ${c.source.run_id}`}
        title={c.title}
        lede={`Score ${score(c.score)} (${WEIGHTS_LABEL}). ${c.lane === "clip" ? "Two or more independent signals agree." : "One signal carries it, or the cut cannot hold its setup: a review case."}`}
      />
      <ChipRow>
        <Chip tone={STATUS_TONE[c.status]}>{STATUS_LABEL[c.status]}</Chip>
        <Chip tone={c.gate.held ? "fail" : "ok"}>{c.gate.held ? `held · ${c.gate.hold_reasons?.length ?? 0}` : "gate clear"}</Chip>
        {c.gate.requires_mandatory_gate && <Chip tone="caution">alleged · evidence checklist</Chip>}
        <Chip tone="muted">{c.segment}</Chip>
        <Chip tone="muted">{c.id}</Chip>
      </ChipRow>
      <WindowBar c={c} />
      <Signals c={c} />
      {c.markers.length > 0 && (
        <Section title="Markers">
          <ul className={styles.plain} data-role="onair-clip-markers">
            {c.markers.map((m, i) => (
              <li key={i}>
                <Chip tone={m.admitted ? "ok" : "muted"}>{m.admitted ? m.kind.replace(/_/g, " ") : `capped · ${m.capped ?? ""}`}</Chip> {m.label} <small>{clock(m.ts_ms)}</small>
                {m.chat_requested === true && (
                  <>
                    {" "}
                    <Chip tone="caution" title="a viewer asked for a clip just before the mark">chat asked</Chip>
                  </>
                )}
              </li>
            ))}
          </ul>
        </Section>
      )}
      {c.evidence && (
        <Section title="Claim context" right={c.evidence.fictional ? "fictional demo case" : undefined}>
          <p className={styles.dim}>
            {c.evidence.case_title} · {c.evidence.case_id} · badge {c.evidence.worst_grade ?? "UNGRADED"}
          </p>
          <ul className={styles.plain}>
            {c.evidence.hexes.map((h) => (
              <li key={h.id}>
                <b>{h.id}</b> {h.label} <Chip tone="muted">{h.grade ?? "ungraded"}</Chip> <small>{(h.sources ?? []).join(" · ") || "no source"}</small>
              </li>
            ))}
          </ul>
          <p className={styles.dim}>Citation shown: {c.evidence.citation.join(" · ") || "none (no hex claim in the cut)"}</p>
        </Section>
      )}
      <Section title="The gate" right={c.gate.requires_mandatory_gate ? "exclusions + evidence checklist" : "exclusions"}>
        <ul className={styles.checks} data-role="onair-clip-gate">
          {c.gate.checks.map((k) => (
            <li key={k.id} data-state={k.state}>
              <Chip tone={GATE_TONE[k.state]}>{k.state.replace(/_/g, " ")}</Chip>
              <b>{k.id.replace(/_/g, " ")}</b>
              {k.mandatory === true && <Chip tone="caution">checklist</Chip>}
              <span>{k.detail}</span>
            </li>
          ))}
        </ul>
      </Section>
      <Section title="Captions" right="the exact transcript lines; viewer handles redacted">
        <ol className={styles.captions}>
          {c.captions.map((l, i) => (
            <li key={i}>
              <small>{secs(l.start_ms)}</small>
              <b>{l.speaker}</b>
              <span>{l.text}</span>
            </li>
          ))}
        </ol>
      </Section>
      <Render c={c} />
      <ClipActions c={c} />
      <Section title="Trail">
        <ul className={styles.plain}>
          {c.history.map((h, i) => (
            <li key={i}>
              <Chip tone={STATUS_TONE[h.status]}>{STATUS_LABEL[h.status]}</Chip> by {h.by} <small>{new Date(h.at_ms).toISOString().slice(0, 16).replace("T", " ")}</small>
              {h.note !== undefined && <span className={styles.dim}> {h.note}</span>}
            </li>
          ))}
          {c.approval && (
            <li>
              Signed by <b>{c.approval.signature}</b>
              {c.approval.override_reason !== undefined && <span className={styles.dim}> · override: {c.approval.override_reason} (over {(c.approval.overridden ?? []).join("; ")})</span>}
            </li>
          )}
          {c.rejection && (
            <li>
              Rejected by <b>{c.rejection.by}</b>: {c.rejection.reason}
            </li>
          )}
        </ul>
      </Section>
    </>
  );
}

function WindowBar({ c }: { c: ClipCandidate }) {
  const w = c.window;
  const bar = windowBar(c);
  const style = { "--pre": String(bar.preroll), "--pay": String(bar.payoff), "--min": String(bar.min) } as CSSProperties;
  return (
    <Section title="The cut" right={`${w.style} · band ${w.band_s[0]}-${w.band_s[1]} s`}>
      <div className={styles.bar} style={style} data-over={bar.over || undefined} data-role="onair-clip-window" aria-label={`Pre-roll ${secs(w.preroll_ms)}, duration ${w.duration_s} s`}>
        <i className={styles.pre} />
        <i className={styles.pay} />
        <em className={styles.min} />
      </div>
      <p className={styles.dim}>
        {w.duration_s.toFixed(1)} s on the spoken clock · pre-roll {secs(w.preroll_ms)} · anchored by {w.driver}
        {w.lag_ms > 0 ? ` (lag ${secs(w.lag_ms)})` : ""} · stream clock {clock(w.start_ms)}–{clock(w.end_ms)} · lines {c.source.line_from}–{c.source.line_to}
      </p>
      {w.needs_reauthoring === true && <p className={styles.warn}>Needs re-authoring: the setup and payoff do not fit the band.</p>}
      {w.note !== undefined && <p className={styles.note}>{w.note}</p>}
    </Section>
  );
}

function Signals({ c }: { c: ClipCandidate }) {
  const s = c.signals;
  const values: Record<string, number | undefined> = { marker: s.marker, chat: s.chat, audio: s.audio, hook: s.hook };
  return (
    <Section title="Signals" right={c.scoring.agreeing.length > 0 ? `agreeing: ${c.scoring.agreeing.join(", ")}` : "no two signals agree"}>
      <table className={styles.signals} data-role="onair-clip-signals">
        <thead>
          <tr>
            <th>Signal</th>
            <th>Value</th>
            <th>Nominal</th>
            <th>Applied</th>
          </tr>
        </thead>
        <tbody>
          {SIGNALS.map((k) => {
            const v = values[k];
            const why = s.unmeasured?.[k];
            return (
              <tr key={k} data-unmeasured={v === undefined || undefined}>
                <td>{k}</td>
                <td>{v === undefined ? <Chip tone="unevaluable" title={why ?? "no reason recorded"}>unmeasured</Chip> : v.toFixed(3)}</td>
                <td>{pct(c.scoring.nominal[k])}</td>
                <td>{pct(c.scoring.effective[k])}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <ul className={styles.plain}>
        {Object.entries(s.unmeasured ?? {}).map(([k, why]) => (
          <li key={k}>
            <b>{k}</b> unmeasured: {why}
          </li>
        ))}
        {s.chat_z !== undefined && <li>crowd z {s.chat_z.toFixed(2)} against the stream’s own baseline</li>}
        {s.hook_why !== undefined && <li>hook read: {s.hook_why}</li>}
      </ul>
      <p className={styles.note}>{c.scoring.provenance}</p>
    </Section>
  );
}

function Render({ c }: { c: ClipCandidate }) {
  const r = c.render;
  if (!r) return null;
  return (
    <Section title="Render" right={`${r.kind} · ${r.width}x${r.height} · ${r.duration_s.toFixed(1)} s`}>
      {r.fallback_reason !== undefined && r.fallback_reason !== "" && <p className={styles.warn}>No video: {r.fallback_reason}</p>}
      <ul className={styles.plain}>
        {r.files.map((f) => (
          <li key={f.path}>
            <b>{f.path}</b> <small>{f.kind} · {f.bytes} B</small>
          </li>
        ))}
      </ul>
      <p className={styles.dim}>
        Burnt in: {r.overlay.alleged_header ? "ALLEGED header · " : ""}
        {r.overlay.grade_badge !== undefined ? `${r.overlay.grade_badge} badge · ` : ""}
        {r.overlay.ai_disclosure ? "AI-character disclosure" : "no disclosure"}. Captions: {r.overlay.captions}
      </p>
    </Section>
  );
}
