// The simulator's canvas palette, captured from A/1 "On Air: The Gallery" (app.js:25 healthColor,
// app.js:420 backdrop, app.js:446-475 the scaled grid). These values are a fidelity role: they
// must match A/1 exactly, so they live here once and every canvas draw reads them from here.
// Proposed to @shadowrunr/tokens as a `sim` group (see the P5 report); until then this is the
// single source and nothing else in the console repeats them.

export const SIM_INK = {
  /** The scene backdrop (A/1 `#070b0a`). */
  backdrop: "#070b0a",
  /** The radial glow over the backdrop (A/1 `smG`, centre stop). */
  glow: "rgba(79,163,141,.12)",
  /** The radial glow's outer stop. */
  glowEnd: "rgba(0,0,0,0)",
  /** A tournament tile at rest. */
  tile: "rgba(255,255,255,.025)",
  /** The hovered tournament tile. */
  tileHover: "rgba(255,177,90,.14)",
  /** The red-line fail dot (A/1's tally red). */
  redLine: "#ff3b2f",
  /** A tile label at rest. */
  label: "rgba(207,195,168,.8)",
  /** The hovered tile's label. */
  labelHover: "#ffb15a",
  /**
   * A run with no verdict yet. A/1's sample had no such state (every replicated run was
   * judged); production runs can be unjudged, so they draw as an unlit slot.
   */
  unjudged: "rgba(255,255,255,.06)",
} as const;

/** A/1's canvas label face. */
export const SIM_FONT = '"Barlow Condensed","Arial Narrow",sans-serif';

/**
 * A/1's health ramp: ember at 0, ochre at 0.5, verdigris at 1. `h` is the share of rule checks
 * that held (0..1), `a` the alpha. Kept byte-identical to A/1 app.js:25.
 */
export function healthColor(h: number, a?: number): string {
  const hue = h < 0.5 ? 14 + h * 52 : 40 + (h - 0.5) * 248;
  const sat = h < 0.5 ? 78 : 62 - (h - 0.5) * 40;
  const light = 50 - Math.abs(h - 0.5) * 8;
  return "hsla(" + hue + "," + sat + "%," + light + "%," + (a == null ? 1 : a) + ")";
}

/** The legend strip under the HUD: the ramp at 0, 0.5 and 1 (A/1 app.js:443). */
export function healthRamp(): string {
  return `linear-gradient(90deg,${healthColor(0)},${healthColor(0.5)},${healthColor(1)})`;
}
