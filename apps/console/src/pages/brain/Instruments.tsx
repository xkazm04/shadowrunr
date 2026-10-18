// A/1's three SVG and lamp instruments (app.js:345-366): the first-word gauge, the worker-pool
// lamps and the picker funnel. Geometry and colours are A/1's, rebuilt as components.
import type { CSSProperties } from "react";
import { TOKENS_BRAIN } from "@shadowrunr/tokens";
import { roleColor } from "./draw";
import type { Funnel, Latency, RoleStat } from "./brainModel";
import { int } from "../../kit/events";
import styles from "./Instruments.module.css";

const SVG_FONT = "Barlow Condensed,Arial Narrow,sans-serif";
/** The gauge's full scale: 8 s. */
export const GAUGE_MAX_MS = 8000;

const ang = (ms: number) => Math.PI * (1 - Math.min(1, ms / GAUGE_MAX_MS));
function at(ms: number, r: number): [string, string] {
  const a = ang(ms);
  return [(160 + r * Math.cos(a)).toFixed(1), (170 - r * Math.sin(a)).toFixed(1)];
}
const pt = (ms: number, r: number) => at(ms, r).join(" ");
const arc = (to: number) => `M${pt(0, 140)} A140 140 0 0 1 ${pt(to, 140)}`;

export function Gauge({ latency }: { latency: Latency }) {
  const b = TOKENS_BRAIN;
  const ticks = Array.from({ length: 9 }, (_, s) => s);
  return (
    <svg viewBox="0 0 320 200" className={styles.gauge} aria-hidden="true" data-role="brain-gauge">
      <path d={arc(GAUGE_MAX_MS)} fill="none" stroke={b.gaugeTrack} strokeWidth="16" />
      {latency.p95 !== undefined && <path d={arc(latency.p95)} fill="none" stroke={b.gaugeP95} strokeWidth="16" />}
      {latency.p50 !== undefined && <path d={arc(latency.p50)} fill="none" stroke={b.gaugeP50} strokeWidth="16" />}
      {ticks.map((s) => {
        const [tx, ty] = at(s * 1000, 116);
        return (
          <g key={s}>
            <path d={`M${pt(s * 1000, 132)} L${pt(s * 1000, 146)}`} stroke={b.gaugeTick} strokeWidth="2" />
            <text x={tx} y={ty} fill={b.gaugeTickText} fontSize="15" textAnchor="middle" fontFamily={SVG_FONT}>
              {s}s
            </text>
          </g>
        );
      })}
      {latency.p50 !== undefined && (
        <g data-role="brain-gauge-needle">
          <path d={`M160 170 L${pt(latency.p50, 128)}`} stroke={b.gaugeNeedle} strokeWidth="4" strokeLinecap="round" />
        </g>
      )}
      <circle cx="160" cy="170" r="8" fill={b.gaugeNeedle} />
    </svg>
  );
}

export function Lamps({ roles }: { roles: readonly RoleStat[] }) {
  const maxC = roles[0]?.calls ?? 1;
  return (
    <div className={styles.lamps} data-role="brain-lamps">
      {roles.map((r, i) => {
        const d = 2.4 + 6.4 * Math.sqrt(r.calls / maxC);
        const style = { "--d": `${d}cqh`, "--lamp": roleColor(r.role), "--blink": `${1.4 + i * 0.5}s` } as CSSProperties;
        return (
          <div key={r.role} className={styles.lampCell} data-role="brain-lamp" data-lamp={r.role}>
            <div className={styles.lamp} style={style} data-error={r.errors > 0} />
            <div className={styles.lampName}>{r.role.replace("_thread", "")}</div>
            <div className={styles.lampCount}>
              {r.calls}
              {r.errors > 0 && (
                <>
                  {" · "}
                  <b className={styles.lampErr}>{r.errors} err</b>
                </>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

export interface FunnelStage {
  readonly n: number;
  readonly label: string;
  readonly color: string;
}

/** Chat in → past moderation → picked → answered (A/1 app.js:362). */
export function funnelStages(f: Funnel): FunnelStage[] {
  const b = TOKENS_BRAIN;
  const [c0, c1, c2, c3] = [b.funnelA, b.funnelB, b.funnelC, b.funnelD];
  return [
    { n: f.chat, label: "chat in", color: c0 },
    { n: f.chat - f.dropped - f.collapsed, label: "past moderation", color: c1 },
    { n: f.picked, label: "picked", color: c2 },
    { n: f.answered, label: "answered", color: c3 },
  ];
}

export function FunnelChart({ funnel }: { funnel: Funnel }) {
  const stages = funnelStages(funnel);
  const total = Math.max(1, funnel.chat);
  const width = (n: number) => 300 * Math.sqrt(Math.max(0, n) / total);
  return (
    <svg viewBox="0 0 320 210" className={styles.funnel} aria-hidden="true" data-role="brain-funnel">
      {stages.map((s, i) => {
        const w0 = width(s.n);
        const next = stages[i + 1];
        const w1 = next ? width(next.n) : w0 * 0.92;
        const y = i * 50;
        const a = 0.28 + i * 0.12;
        return (
          <g key={s.label} data-stage={s.label}>
            <path d={`M${160 - w0 / 2} ${y} L${160 + w0 / 2} ${y} L${160 + w1 / 2} ${y + 44} L${160 - w1 / 2} ${y + 44}Z`} fill={s.color} opacity={a}>
              <animate attributeName="opacity" values={`${a};${a + 0.17};${a}`} dur={`${2 + i * 0.4}s`} repeatCount="indefinite" />
            </path>
            <text x="160" y={y + 29} textAnchor="middle" fill={TOKENS_BRAIN.funnelText} fontSize="19" fontFamily={SVG_FONT} letterSpacing="1">
              {int(s.n)} {s.label.toUpperCase()}
            </text>
          </g>
        );
      })}
    </svg>
  );
}
