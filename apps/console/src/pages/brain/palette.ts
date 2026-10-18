// A/1's brain oscilloscope palette ("On Air: The Gallery", app.js:20 and 333-391). A fidelity
// role captured from A/1, so every value is kept exactly. Canvas and SVG read these directly; the
// module CSS reads the text and panel colours through the `--br-*` properties `paletteVars()`
// sets on the layer-1 root (component CSS carries no raw colour).
//
// Proposal for the Director: admit these under a scoped `brain-*` token group (like `screen-*`),
// then this module re-exports the token mirror instead of holding literals.
import type { CSSProperties } from "react";
import type { Role } from "@shadowrunr/contract";

/** A/1 `ROLEC`: one colour per engine role. Roles A/1 never saw fall back to `unknownRole`. */
export const ROLE_COLOR: Readonly<Partial<Record<Role, string>>> = {
  core_thread: "#ff6a2b",
  chatter: "#86d3bb",
  researcher: "#ffb15a",
  consolidator: "#cdbf9f",
  judge: "#c9a6ff",
  chat_picker: "#9fd3ff",
};

export const BRAIN = {
  unknownRole: "#ccc",
  backdrop: "#070b0a",
  glowCore: "rgba(255,106,43,.08)",
  glowEdge: "rgba(0,0,0,0)",
  panelEdge: "rgba(134,211,187,.18)",
  panelFill: "rgba(3,7,6,.6)",
  scope: {
    grid: "rgba(134,211,187,.07)",
    laneLabel: "rgba(154,146,132,.9)",
    trace: "rgba(255,106,43,.75)",
    traceGlow: "#ff6a2b",
    error: "#ff3b2f",
    sweepFrom: "rgba(255,177,90,0)",
    sweepTo: "rgba(255,177,90,.18)",
  },
  /** Memory curves, one per lane (A/1 indexed four runs; more lanes cycle). */
  memory: ["#86d3bb", "#4fa38d", "#ffb15a", "#ff6a2b"],
  gauge: {
    track: "rgba(134,211,187,.12)",
    p95: "rgba(255,177,90,.35)",
    p50: "#4fa38d",
    tick: "rgba(207,195,168,.5)",
    tickText: "#9a9284",
    needle: "#ff6a2b",
  },
  funnel: ["#86d3bb", "#4fa38d", "#ffb15a", "#ff6a2b"],
  funnelText: "#efe6d2",
  lampHighlight: "#fff8",
  lampError: "#ff3b2f",
  text: {
    bone: "#efe6d2",
    bone2: "#cfc3a8",
    ash: "#9a9284",
    verd2: "#86d3bb",
    ember2: "#ffb15a",
    errorText: "#ff8a65",
  },
  door: { fill: "rgba(6,12,11,.72)", edge: "rgba(127,190,170,.22)" },
} as const;

const BY_NAME: Readonly<Record<string, string | undefined>> = ROLE_COLOR;

export function roleColor(role: string): string {
  return BY_NAME[role] ?? BRAIN.unknownRole;
}

/** The `--br-*` custom properties the layer-1 CSS reads. */
export function paletteVars(): CSSProperties {
  const t = BRAIN.text;
  const vars: Record<string, string> = {
    "--br-backdrop": BRAIN.backdrop,
    "--br-panel-edge": BRAIN.panelEdge,
    "--br-panel-fill": BRAIN.panelFill,
    "--br-bone": t.bone,
    "--br-bone2": t.bone2,
    "--br-ash": t.ash,
    "--br-verd2": t.verd2,
    "--br-ember2": t.ember2,
    "--br-error-text": t.errorText,
    "--br-lamp-highlight": BRAIN.lampHighlight,
    "--br-lamp-error": BRAIN.lampError,
    "--br-door-fill": BRAIN.door.fill,
    "--br-door-edge": BRAIN.door.edge,
    "--br-sweep-from": BRAIN.scope.sweepFrom,
    "--br-sweep-to": BRAIN.scope.sweepTo,
  };
  return vars;
}
