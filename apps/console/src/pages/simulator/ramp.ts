// A/1's health ramp (app.js:25 healthColor, app.js:443 the legend strip): ember at 0, ochre at
// 0.5, verdigris at 1. A computed colour, not a token: the hue moves with the share held. The
// grid's flat inks are the `sim-*` tokens.

/** `h` is the share of rule checks that held (0..1), `a` the alpha. Byte-identical to A/1. */
export function healthColor(h: number, a?: number): string {
  const hue = h < 0.5 ? 14 + h * 52 : 40 + (h - 0.5) * 248;
  const sat = h < 0.5 ? 78 : 62 - (h - 0.5) * 40;
  const light = 50 - Math.abs(h - 0.5) * 8;
  return "hsla(" + hue + "," + sat + "%," + light + "%," + (a == null ? 1 : a) + ")";
}

/** The legend strip under the HUD: the ramp at 0, 0.5 and 1. */
export function healthRamp(): string {
  return `linear-gradient(90deg,${healthColor(0)},${healthColor(0.5)},${healthColor(1)})`;
}
