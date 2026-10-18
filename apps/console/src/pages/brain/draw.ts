// A/1's oscilloscope and memory canvases (app.js:370-390), drawn once per input change. A/1
// redrew every frame for its sweep; here the sweep is a CSS overlay and the pulse dots are drawn
// at A/1's reduced-motion state (`sweep = 1`), so nothing runs per tick.
import { BRAIN, roleColor } from "./palette";
import type { Lane, MemoryCurve } from "./brainModel";

/** The slice of CanvasRenderingContext2D the brain draws with (tests pass a recorder). */
export type Ctx = Pick<
  CanvasRenderingContext2D,
  | "clearRect"
  | "beginPath"
  | "moveTo"
  | "lineTo"
  | "stroke"
  | "fill"
  | "arc"
  | "fillText"
  | "strokeStyle"
  | "fillStyle"
  | "lineWidth"
  | "font"
  | "shadowColor"
  | "shadowBlur"
  | "globalAlpha"
>;

export interface Size {
  readonly w: number;
  readonly h: number;
}

const LANE_FONT = '"Barlow Condensed","Arial Narrow",sans-serif';
/** A/1's reduced-motion sweep position: every dot is drawn as if the sweep just passed. */
const SWEEP = 1;

export function laneX(t: number, span: number, w: number): number {
  return Math.min(0.99, t / span) * w * 0.98 + w * 0.01;
}

export function drawScope(c: Ctx, { w, h }: Size, lanes: readonly Lane[]): void {
  c.clearRect(0, 0, w, h);
  const top = h * 0.3;
  const laneH = (h - top - h * 0.05) / Math.max(1, lanes.length);
  const s = BRAIN.scope;
  c.strokeStyle = s.grid;
  c.lineWidth = 1;
  for (let gx = 0; gx <= 20; gx++) {
    c.beginPath();
    c.moveTo((gx * w) / 20, top - 10);
    c.lineTo((gx * w) / 20, h);
    c.stroke();
  }
  lanes.forEach((lane, li) => {
    const y0 = top + li * laneH + laneH * 0.62;
    c.fillStyle = s.laneLabel;
    c.font = `600 ${Math.max(11, laneH * 0.2)}px ${LANE_FONT}`;
    c.fillText(lane.label.toUpperCase(), 8, y0 - laneH * 0.42);
    c.beginPath();
    c.moveTo(0, y0);
    for (const p of lane.pulses) {
      const x = laneX(p.t, lane.span, w);
      const amp = Math.min(laneH * 0.55, Math.log(1 + p.dur / 300) * laneH * 0.09) * (p.role === "core_thread" ? 1 : -0.6);
      c.lineTo(x - 3, y0);
      c.lineTo(x, y0 - amp);
      c.lineTo(x + 3, y0 + amp * 0.3);
      c.lineTo(x + 6, y0);
    }
    c.lineTo(w, y0);
    c.strokeStyle = s.trace;
    c.lineWidth = 1.6;
    c.shadowColor = s.traceGlow;
    c.shadowBlur = 8;
    c.stroke();
    c.shadowBlur = 0;
    for (const p of lane.pulses) {
      const x = laneX(p.t, lane.span, w);
      let age = SWEEP - x / w;
      if (age < 0) age = 1;
      c.fillStyle = roleColor(p.role);
      c.globalAlpha = Math.max(0.3, 1 - age * 3);
      c.beginPath();
      c.arc(x, y0 + laneH * 0.3, p.err ? 4 : 2.2, 0, 7);
      c.fill();
      c.globalAlpha = 1;
      if (p.err) {
        c.strokeStyle = s.error;
        c.beginPath();
        c.arc(x, y0 + laneH * 0.3, 7, 0, 7);
        c.stroke();
      }
    }
  });
}

export function drawMemory(c: Ctx, { w, h }: Size, curves: readonly (MemoryCurve | undefined)[], memMax: number): void {
  c.clearRect(0, 0, w, h);
  curves.forEach((curve, li) => {
    if (!curve) return;
    c.beginPath();
    c.moveTo(0, h);
    for (const p of curve.points) c.lineTo((p.t / curve.span) * w, h - (p.n / memMax) * h * 0.62);
    c.strokeStyle = BRAIN.memory[li % BRAIN.memory.length] ?? BRAIN.unknownRole;
    c.lineWidth = 2;
    c.stroke();
  });
}
