// Pure helpers for the Characters page: the roster window, display names, the generic avatar's
// deterministic spec (the winner's sr-kit `avatarSpec`, kept exactly), and the situation
// verdict tallies read out of a tournament's results.json.
import type { CharacterSummary, FoilInfo, TournamentResults } from "@shadowrunr/contract";

/** The roster switches to the dense "billing block" (two rows of ten) above this many. */
export const ROSTER_MANY = 6;
/** Tiles the poster roster ever draws; past this a "+N more" tile opens the table. */
export const ROSTER_MAX = 20;

export interface RosterWindow {
  readonly start: number;
  readonly end: number;
  /** Characters not drawn (shown as the "+N more" tile). */
  readonly more: number;
}

/**
 * Which characters the poster roster draws: everything up to ROSTER_MAX, otherwise
 * ROSTER_MAX - 1 tiles around the selected one plus a "+N more" tile. The selection always stays
 * inside the window.
 */
export function rosterWindow(count: number, selected: number, max = ROSTER_MAX): RosterWindow {
  if (count <= max) return { start: 0, end: count, more: 0 };
  const shown = max - 1;
  const at = Math.min(Math.max(selected, 0), count - 1);
  const start = Math.min(Math.max(at - Math.floor(shown / 2), 0), count - shown);
  return { start, end: start + shown, more: count - shown };
}

/** The selected character's index: the remembered id if it is listed, else the first. */
export function selectedIndex(rows: readonly CharacterSummary[], id: string | undefined): number {
  const i = id === undefined ? -1 : rows.findIndex((r) => r.id === id);
  return i < 0 ? 0 : i;
}

/** Arrow-key step through the roster, wrapping at both ends (the winner's roster keys). */
export function stepIndex(i: number, delta: 1 | -1, count: number): number {
  return count === 0 ? 0 : (i + delta + count) % count;
}

/** `01 of 1 · the first character` (the name plate kicker). */
export function rosterKicker(i: number, count: number): string {
  const n = (i + 1).toLocaleString("en-US").padStart(2, "0");
  return `${n} of ${count.toLocaleString("en-US")}${i === 0 ? " · the first character" : ""}`;
}

const TITLES = /^(The|Sir|Dame|Lady|Sister|Brother|Baron|Captain|Madame|Professor|Warden|Countess|Grandmother|Arch-Lich) /;

/** The roster tile's short name: titles dropped, first word kept (`Arch-Lich Malakor` → `Malakor`). */
export function shortName(name: string): string {
  return name.replace(TITLES, "").split(" ")[0] ?? name;
}

/** `weary-middle-manager` → `Weary middle manager`. */
export function toneLabel(id: string): string {
  const s = id.replace(/[-_]+/g, " ").trim();
  return s.length === 0 ? id : s.charAt(0).toUpperCase() + s.slice(1);
}

/** The heading of a package document after its kind: `# Foil: The Disruptor` → `The Disruptor`. */
export function docHeading(text: string, kind: string): string | undefined {
  const m = new RegExp(`^#\\s+${kind}:\\s*(.+)$`, "m").exec(text);
  return m?.[1]?.trim();
}

/** A foil's display name: its `# Foil:` heading, else the id humanised. */
export function foilName(foil: Pick<FoilInfo, "id" | "text"> | { readonly id: string; readonly text?: undefined }): string {
  const h = foil.text === undefined ? undefined : docHeading(foil.text, "Foil");
  if (h !== undefined) return h;
  return toneLabel(foil.id.replace(/^the-/, "the "))
    .split(" ")
    .map((w) => (w.length > 0 ? w.charAt(0).toUpperCase() + w.slice(1) : w))
    .join(" ");
}

/** The foil strip label: the leading article dropped (`The Disruptor` → `Disruptor`). */
export function foilShort(name: string): string {
  return name.replace(/^The /, "");
}

/** The name plate's facts line, built only from what `character.list` reports. */
export function factsLine(c: CharacterSummary): string {
  const parts = [`${c.tones.length} ${c.tones.length === 1 ? "tone" : "tones"}`, `${c.foils.length} ${c.foils.length === 1 ? "foil" : "foils"}`];
  if (c.rules_total > 0) parts.push(`${c.rules_enabled} of ${c.rules_total} optional rules on`);
  return parts.join(" · ");
}

// ---- the generic avatar (sr-kit avatarSpec, kept exactly) ----

/** FNV-1a, 32 bit (sr-kit SR.hash). */
export function hash32(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** Xorshift32 in [0, 1) (sr-kit SR.rng). */
export function rng(seed: number): () => number {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return ((s >>> 0) % 4294967296) / 4294967296;
  };
}

export interface AvatarSpec {
  readonly head: number;
  readonly gear: number;
  readonly eyes: number;
  readonly mark: number;
  readonly hue: number;
  readonly accent: string;
  readonly accent2: string;
  readonly deep: string;
  readonly tilt: number;
}

const HUES = [158, 168, 28, 44, 12, 196, 330, 262] as const;

export function avatarSpec(seed: number): AvatarSpec {
  const r = rng(seed);
  const hue = HUES[Math.floor(r() * 8)] ?? HUES[0];
  const head = Math.floor(r() * 6);
  const gear = Math.floor(r() * 7);
  const eyes = Math.floor(r() * 5);
  const mark = Math.floor(r() * 5);
  return {
    head,
    gear,
    eyes,
    mark,
    hue,
    accent: `hsl(${hue},55%,58%)`,
    accent2: `hsl(${(hue + 28) % 360},70%,64%)`,
    deep: `hsl(${hue},38%,22%)`,
    tilt: (r() - 0.5) * 6,
  };
}

/** A non-Malakor character's avatar, seeded from its id as the winner seeded from the name. */
export const avatarOf = (id: string): AvatarSpec => avatarSpec(hash32(id) + 7);

// ---- situation verdicts out of results.json ----

export interface SituationTally {
  readonly pass: number;
  readonly fail: number;
  readonly not_provoked: number;
  readonly not_evaluable: number;
  /** `<run_id>#<line>` refs of failing evidence, in cell order, deduplicated. */
  readonly failRefs: readonly string[];
}

export const SITUATION_PREFIX = "situation:";

type Rec = Readonly<Record<string, unknown>>;
const isRec = (v: unknown): v is Rec => typeof v === "object" && v !== null && !Array.isArray(v);
const count = (v: unknown): number => (typeof v === "number" && Number.isFinite(v) ? v : 0);

/**
 * Sums every cell's `situation:*` rule tallies, keyed by situation id. `tournament.results` is
 * an open record in the contract, so this narrows only the fields it reads; a reply without a
 * `cells` array is `null` (shown as degraded).
 */
export function situationTallies(raw: TournamentResults): Map<string, SituationTally> | null {
  const cells = raw["cells"];
  if (!Array.isArray(cells)) return null;
  const acc = new Map<string, { pass: number; fail: number; not_provoked: number; not_evaluable: number; refs: Set<string> }>();
  for (const cell of cells) {
    if (!isRec(cell)) continue;
    const rules = cell["rules"];
    if (!isRec(rules)) continue;
    for (const [rule, t] of Object.entries(rules)) {
      if (!rule.startsWith(SITUATION_PREFIX) || !isRec(t)) continue;
      const id = rule.slice(SITUATION_PREFIX.length);
      const a = acc.get(id) ?? { pass: 0, fail: 0, not_provoked: 0, not_evaluable: 0, refs: new Set<string>() };
      a.pass += count(t["pass"]);
      a.fail += count(t["fail"]);
      a.not_provoked += count(t["not_provoked"]);
      a.not_evaluable += count(t["not_evaluable"]);
      const refs = t["fail_refs"];
      if (Array.isArray(refs)) for (const r of refs) if (typeof r === "string") a.refs.add(r);
      acc.set(id, a);
    }
  }
  const out = new Map<string, SituationTally>();
  for (const [id, a] of acc) {
    out.set(id, { pass: a.pass, fail: a.fail, not_provoked: a.not_provoked, not_evaluable: a.not_evaluable, failRefs: [...a.refs] });
  }
  return out;
}

/** Runs where the situation was judged at all (pass + fail). */
export const judged = (t: SituationTally | undefined): number => (t ? t.pass + t.fail : 0);

/** `<run_id>#<line>` → its parts; a ref without a line keeps only the run. */
export function parseRef(ref: string): { readonly run: string; readonly line?: string } {
  const at = ref.lastIndexOf("#");
  if (at <= 0) return { run: ref };
  const line = ref.slice(at + 1);
  return /^\d+$/.test(line) ? { run: ref.slice(0, at), line } : { run: ref };
}

/** Distinct run ids of a tally's failing evidence, in order. */
export function failingRuns(t: SituationTally | undefined): string[] {
  if (!t) return [];
  return [...new Set(t.failRefs.map((r) => parseRef(r).run))];
}
