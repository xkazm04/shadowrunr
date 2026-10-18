/*
 * GENERATED FILE. Do not edit by hand.
 * Source: packages/tokens/tokens.source.json
 * Regenerate: node packages/tokens/scripts/generate.mjs
 * Drift gate: node packages/tokens/scripts/generate.mjs --check
 */

/** The `color` roles resolved to their values. */
export const TOKENS_COLOR = {
  surfaceVoid: "#0b0f0e",
  surfacePaper: "#efe6d2",
  surfaceChip: "#e3d6b8",
  surfaceHoverPaper: "#d4c5a1",
  surfaceCard: "#f6efdc",
  surfaceField: "#faf4e4",
  surfaceCaution: "#f3dca6",
  surfaceSelected: "#fbe6d6",
  surfacePress: "#14110d",
  textOnDark: "#efe6d2",
  textInk: "#161411",
  textMute: "#6b5f4a",
  textNote: "#3a342a",
  textOnPress: "#e8dfc7",
  textOnCaution: "#2d2410",
  textOnSignal: "#fff",
  textMuteOnDark: "#cdbf9f",
  linkOnPress: "#ffb15a",
  chromeInk: "#0b0f0e",
  signal: "#e8542a",
  signalDeep: "#b53a14",
  ok: "#1b6150",
  okBright: "#86d3bb",
  fail: "#8e2a1a",
  cautionEdge: "#7d5f12",
  foil: "#d6a85a",
  maskInk: "#000",
  verdictHeldGround: "#bfe0d3",
  verdictHeldEdge: "#1b6150",
  verdictHeldInk: "#0f3a2f",
  verdictFailedGround: "#f0b9a5",
  verdictFailedEdge: "#8e2a1a",
  verdictFailedInk: "#5a1708",
  verdictWobbledGround: "#f3dca6",
  verdictWobbledEdge: "#7d5f12",
  verdictUnevaluableGround: "#e0d6ff",
  verdictUnevaluableEdge: "#5a4a9a",
  verdictUnevaluableInk: "#2a2060",
  diffAdd: "#86d3bb",
  diffDel: "#ff9c82",
  screenGround: "#0d1312",
  screenSetFar: "#16231f",
  screenSetNear: "#1c2e29",
  screenHex: "#1a2421",
  screenChat: "#e5dcc7",
  scrim: "#070a09",
} as const;
/** The `alpha` roles resolved to their values. */
export const TOKENS_ALPHA = {
  line: "rgba(20,17,13,.2)",
  wash: "rgba(20,17,13,.06)",
  track: "rgba(20,17,13,.14)",
  rowHover: "rgba(232,84,42,.12)",
  rowSelected: "rgba(232,84,42,.2)",
  shadowSeam: "rgba(0,0,0,.35)",
} as const;
/** The `font` roles resolved to their values. */
export const TOKENS_FONT = {
  display: "\"Barlow Condensed\",\"Bahnschrift SemiCondensed\",\"Arial Narrow\",\"Roboto Condensed\",sans-serif",
  serif: "\"Palatino Linotype\",Palatino,\"Iowan Old Style\",\"Book Antiqua\",Georgia,serif",
  sans: "\"Segoe UI\",system-ui,-apple-system,\"Helvetica Neue\",Arial,sans-serif",
  mono: "\"Cascadia Mono\",Consolas,\"SFMono-Regular\",Menlo,monospace",
} as const;
/** The `weight` roles resolved to their values. */
export const TOKENS_WEIGHT = {
  regular: "400",
  medium: "500",
  semibold: "600",
  bold: "700",
} as const;
/** The `size` roles resolved to their values. */
export const TOKENS_SIZE = {
  micro: ".8125rem",
  caption: ".875rem",
  small: ".9375rem",
  body: "1rem",
  control: "1.05rem",
  label: "1.1rem",
  labelLg: "1.15rem",
  kicker: "1.2rem",
  kickerLg: "1.25rem",
  lede: "1.3rem",
  subhead: "1.4rem",
  subheadLg: "1.5rem",
  title: "1.7rem",
  figure: "2rem",
  heading: "2.4rem",
  headingLg: "2.6rem",
} as const;
/** The `root` roles resolved to their values. */
export const TOKENS_ROOT = {
  size: "clamp(16px,min(1.25vw,2vh),30.4px)",
} as const;
/** The `space` roles resolved to their values. */
export const TOKENS_SPACE = {
  hair: ".1rem",
  tick: ".2rem",
  snug: ".3rem",
  tight: ".4rem",
  compact: ".5rem",
  cozy: ".6rem",
  close: ".7rem",
  near: ".8rem",
  base: "1rem",
  loose: "1.2rem",
  roomy: "1.4rem",
  wide: "1.6rem",
  section: "2rem",
  gutter: "2.4rem",
} as const;
/** The `stroke` roles resolved to their values. */
export const TOKENS_STROKE = {
  hair: "1px",
  rule: "2px",
  heavy: "3px",
} as const;
/** The `radius` roles resolved to their values. */
export const TOKENS_RADIUS = {
  none: "0",
  nick: "2px",
  round: "50%",
} as const;
/** The `z` roles resolved to their values. */
export const TOKENS_Z = {
  under: "0",
  above: "1",
  wall: "2",
  stickyHead: "2",
  posterBody: "3",
  posterSpine: "4",
  spread: "5",
  railStrip: "6",
  railOpen: "7",
  posterHit: "8",
  posterGrain: "9",
  spreadTabs: "20",
  stageTitle: "30",
  chrome: "60",
  intro: "80",
  toast: "90",
  screenFigure: "1",
  screenPanel: "2",
  screenCaption: "3",
  screenFoil: "4",
  screenCard: "5",
  screenDecree: "6",
  screenSlate: "8",
} as const;
/** The `ease` roles resolved to their values. */
export const TOKENS_EASE = {
  move: "cubic-bezier(.7,0,.2,1)",
  settle: "cubic-bezier(.2,.8,.2,1)",
  slam: "cubic-bezier(.2,.95,.25,1.12)",
} as const;
/** The `breakpoint` roles resolved to their values. */
export const TOKENS_BREAKPOINT = {
  narrow: "760px",
  split: "1100px",
  aside: "1250px",
} as const;
/** The `texture` roles resolved to their values. */
export const TOKENS_TEXTURE = {
  grainImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='180' height='180'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='.85' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix values='0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1.5 0 0 0 -.5'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E\")",
  grainBlend: "multiply",
  grainOpacityPoster: ".4",
  grainOpacitySpread: ".32",
  halftoneACell: ".8rem",
  halftoneADot: "26%",
  halftoneAEdge: "28%",
  halftoneAOpacity: ".55",
  halftoneAAngle: "160deg",
  halftoneAFade: "62%",
  halftoneADrift: "26s",
  halftoneBCell: "1.1rem",
  halftoneBDot: "22%",
  halftoneBEdge: "24%",
  halftoneBOpacity: ".35",
  halftoneBAngle: "20deg",
  halftoneBFade: "55%",
  halftoneBDrift: "38s",
  halftoneDriftX: ".8rem",
  halftoneDriftY: "1.6rem",
  halftoneSunCell: "1rem",
  halftoneSunDot: "30%",
  halftoneSunEdge: "33%",
  halftoneSunOpacity: ".8",
} as const;
/** The `layout` roles resolved to their values. */
export const TOKENS_LAYOUT = {
  spine: "18.75rem",
  strip: "3.6rem",
  railPeek: "11rem",
  spreadMeasure: "84rem",
  bodyMeasure: "64rem",
} as const;
/** The `brain` roles resolved to their values. */
export const TOKENS_BRAIN = {
  roleCoreThread: "#ff6a2b",
  roleChatter: "#86d3bb",
  roleResearcher: "#ffb15a",
  roleConsolidator: "#cdbf9f",
  roleJudge: "#c9a6ff",
  roleChatPicker: "#9fd3ff",
  roleUnknown: "#ccc",
  backdrop: "#070b0a",
  glowCore: "rgba(255,106,43,.08)",
  glowEdge: "rgba(0,0,0,0)",
  panelEdge: "rgba(134,211,187,.18)",
  panelFill: "rgba(3,7,6,.6)",
  scopeGrid: "rgba(134,211,187,.07)",
  scopeLaneLabel: "rgba(154,146,132,.9)",
  scopeTrace: "rgba(255,106,43,.75)",
  scopeTraceGlow: "#ff6a2b",
  scopeError: "#ff3b2f",
  scopeSweepFrom: "rgba(255,177,90,0)",
  scopeSweepTo: "rgba(255,177,90,.18)",
  memoryA: "#86d3bb",
  memoryB: "#4fa38d",
  memoryC: "#ffb15a",
  memoryD: "#ff6a2b",
  gaugeTrack: "rgba(134,211,187,.12)",
  gaugeP95: "rgba(255,177,90,.35)",
  gaugeP50: "#4fa38d",
  gaugeTick: "rgba(207,195,168,.5)",
  gaugeTickText: "#9a9284",
  gaugeNeedle: "#ff6a2b",
  funnelA: "#86d3bb",
  funnelB: "#4fa38d",
  funnelC: "#ffb15a",
  funnelD: "#ff6a2b",
  funnelText: "#efe6d2",
  lampHighlight: "#fff8",
  lampError: "#ff3b2f",
  textBone: "#efe6d2",
  textBoneDim: "#cfc3a8",
  textAsh: "#9a9284",
  textVerdigris: "#86d3bb",
  textApricot: "#ffb15a",
  textError: "#ff8a65",
  doorFill: "rgba(6,12,11,.72)",
  doorEdge: "rgba(127,190,170,.22)",
} as const;
/** The `sim` roles resolved to their values. */
export const TOKENS_SIM = {
  backdrop: "#070b0a",
  glow: "rgba(79,163,141,.12)",
  glowEnd: "rgba(0,0,0,0)",
  tile: "rgba(255,255,255,.025)",
  tileHover: "rgba(255,177,90,.14)",
  redLine: "#ff3b2f",
  label: "rgba(207,195,168,.8)",
  labelHover: "#ffb15a",
  unjudged: "rgba(255,255,255,.06)",
  font: "\"Barlow Condensed\",\"Arial Narrow\",sans-serif",
} as const;

/** Every role resolved to its value (canvas and inline use). Primitives stay in the source JSON. */
export const TOKENS = {
  color: TOKENS_COLOR,
  alpha: TOKENS_ALPHA,
  font: TOKENS_FONT,
  weight: TOKENS_WEIGHT,
  size: TOKENS_SIZE,
  root: TOKENS_ROOT,
  space: TOKENS_SPACE,
  stroke: TOKENS_STROKE,
  radius: TOKENS_RADIUS,
  z: TOKENS_Z,
  ease: TOKENS_EASE,
  breakpoint: TOKENS_BREAKPOINT,
  texture: TOKENS_TEXTURE,
  layout: TOKENS_LAYOUT,
  brain: TOKENS_BRAIN,
  sim: TOKENS_SIM,
} as const;

/** CSS custom-property prefix per role group; a role's property is prefix + kebab-case key. */
export const VAR_PREFIX = {
  color: "--sr-color-",
  alpha: "--sr-color-",
  font: "--sr-font-",
  weight: "--sr-weight-",
  size: "--sr-size-",
  root: "--sr-root-",
  space: "--sr-space-",
  stroke: "--sr-stroke-",
  radius: "--sr-radius-",
  z: "--sr-z-",
  ease: "--sr-ease-",
  breakpoint: "--sr-bp-",
  texture: "--sr-",
  layout: "--sr-layout-",
  brain: "--sr-brain-",
  sim: "--sr-sim-",
} as const;

/** Values that change at a breakpoint, keyed by breakpoint role then CSS custom property. */
export const AT_BREAKPOINT = {
  narrow: {
    "--sr-root-size": "16px",
  },
} as const;

export const PAGE_IDS = ["onair", "characters", "brain", "segments", "simulator", "reconcile"] as const;
export type PageId = (typeof PAGE_IDS)[number];

/** Per-page palettes; tokens.css scopes them under [data-page="<id>"]. */
export const PAGE_PALETTES = {
  onair: {
    face: "#e8542a",
    type: "#0b0f0e",
    overprint: "#2f7d6a",
    overprint2: "#efe6d2",
    key: "#0b0f0e",
    accent: "#e8542a",
    accentText: "#b53a14",
  },
  characters: {
    face: "#efe6d2",
    type: "#0b0f0e",
    overprint: "#e8542a",
    overprint2: "#2f7d6a",
    key: "#0b0f0e",
    accent: "#2f7d6a",
    accentText: "#1b6150",
  },
  brain: {
    face: "#14463d",
    type: "#efe6d2",
    overprint: "#e8542a",
    overprint2: "#86d3bb",
    key: "#06110e",
    accent: "#3a9b86",
    accentText: "#1b6150",
  },
  segments: {
    face: "#d6a85a",
    type: "#0b0f0e",
    overprint: "#a8321c",
    overprint2: "#efe6d2",
    key: "#0b0f0e",
    accent: "#d6a85a",
    accentText: "#7d5f12",
  },
  simulator: {
    face: "#8e2a1a",
    type: "#efe6d2",
    overprint: "#d6a85a",
    overprint2: "#e8542a",
    key: "#0b0f0e",
    accent: "#c9452c",
    accentText: "#8e2a1a",
  },
  reconcile: {
    face: "#0b0f0e",
    type: "#efe6d2",
    overprint: "#2f7d6a",
    overprint2: "#e8542a",
    key: "#0b0f0e",
    accent: "#e8542a",
    accentText: "#b53a14",
  },
} as const;
export const PAGE_VARS = {
  face: "--sr-page-face",
  type: "--sr-page-type",
  overprint: "--sr-page-overprint",
  overprint2: "--sr-page-overprint-2",
  key: "--sr-page-key",
  accent: "--sr-page-accent",
  accentText: "--sr-page-accent-text",
} as const;

/** Viewer rank colours (fidelity role, single source for the console and the stage). */
export const RANK_COLORS = {
  thrall: "#8f877a",
  acolyte: "#cdbf9f",
  cultist: "#8fc4b0",
  warlock: "#3f9c86",
  lich: "#e8913f",
  dread_council: "#ff5a2a",
} as const;

/** Evidence grade colours (fidelity role, single source for the console and the stage). */
export const GRADE_COLORS = {
  DOC: "#86d3bb",
  REP: "#cdbf9f",
  CLM: "#e8913f",
  SPC: "#ff5a2a",
} as const;

/** Breakpoints in CSS px, for matchMedia and canvas layout. */
export const BREAKPOINTS_PX = {
  narrow: 760,
  split: 1100,
  aside: 1250,
} as const;

/** The only media queries components may use (breakpoint roles plus reduced motion). */
export const MEDIA = {
  narrow: "(max-width: 760px)",
  aboveNarrow: "(min-width: 761px)",
  split: "(max-width: 1100px)",
  aboveSplit: "(min-width: 1101px)",
  aside: "(max-width: 1250px)",
  aboveAside: "(min-width: 1251px)",
  reducedMotion: "(prefers-reduced-motion: reduce)",
} as const;
