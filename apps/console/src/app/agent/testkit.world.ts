// The testkit's realistic daemon (tests only): every method a tool sends, answered at the size
// a real season reaches, so the size guard measures what Athena would see. A 150-line session
// with long host lines, judged and marked with long notes; the real 13.5k-char Malakor bible;
// a 3,000-entry ledger; 36-cell tournament results; a dozen characters and 40 package issues.
import type { EngineStatus, JudgeVerdict, LedgerEntry, LineFeedback, ReviewSession, RpcMethod, RpcResult, RunRow, SandboxStatus, StreamEvent, TournamentRow } from "@shadowrunr/contract";
import { genResults, genRoster, malakorParams, malakorRules } from "../../pages/characters/testgen";
import { ledgerRows, ledgerSummary } from "../../pages/brain/testgen";
import { sessionRows } from "../../pages/review/fixture";
import { genRanks, genReport, genTables } from "../../pages/segments/testgen";
import { ok, type Reply, type Script } from "./testkit";

export const BIG_SESSION = "sbx-20261020-221153-5m";
export const BIG_RUN = "pilot-d1ef98d3a6/theatrical.haiku.SEG-01.a30.s1";
export const LINES = 150;

const long = (n: number, seed: string) => {
  let s = "";
  while (s.length < n) s += `${seed} minions, the Grudge Ledger records "every" late arrival\\and I keep it open. `;
  return s.slice(0, n);
};

/** 150 lines: a host line on four in five (about 330 chars), chat between (about 140). */
export function bigEvents(id = BIG_SESSION): StreamEvent[] {
  const out: StreamEvent[] = [{ type: "stream_start", ts_ms: 0, stream_id: id, character: "malakor", tone_variant: "unhinged-roastmaster", core_tier: "sonnet", audience_size: 30 }];
  for (let i = 1; i < LINES; i++) {
    const ts_ms = i * 2000;
    if (i % 5 === 0) {
      out.push({ type: "chat", ts_ms, msg: { id: `c${i}`, text: long(140, `chat ${i}`), ts_ms, viewer: { id: `v${i % 30}`, name: `FogKettle${i % 30}`, platform: "sim", rank: "cultist" } } });
    } else out.push({ type: "say", ts_ms, speaker: "host", text: long(330, `Line ${i}`) });
  }
  return out;
}

export function bigVerdict(run = BIG_SESSION): JudgeVerdict {
  const ref = (k: number) => `${run}#${1 + ((k * 7) % (LINES - 1))}`;
  return {
    run_id: run,
    judge_tier: "opus",
    notes: long(900, "Judge"),
    rules: Array.from({ length: 24 }, (_, k) => ({
      rule: k < 6 ? `situation:a-rather-long-situation-name-${k}` : `red-line:rule-${k}`,
      state: k < 6 ? ("fail" as const) : ("pass" as const),
      ...(k < 6 ? { evidence: Array.from({ length: 12 }, (_, e) => ref(k * 12 + e)) } : {}),
    })),
    quality: Array.from({ length: 30 }, (_, k) => ({ dimension: `dimension_${k % 8}`, value: k % 3 !== 0, quote: long(120, "quote"), turn_ref: ref(k) })),
  };
}

const feedback = (session: string): LineFeedback[] =>
  Array.from({ length: 14 }, (_, k) => ({ session_id: session, line: 2 + k * 9, tags: ["flat", "off_tone", "repetitive"], note: long(1800, `Note ${k}`), at_ms: 1_792_500_100_000 + k }));

export function bigSessionOf(id = BIG_SESSION): ReviewSession {
  const events = bigEvents(id);
  const [row] = sessionRows(1);
  if (row === undefined) throw new Error("no row");
  return {
    row: { ...row, session_id: id, lines: LINES, rule_fails: 6, checks: row.checks.map((c) => ({ ...c, detail: long(180, c.id) })) },
    segments: [
      { segment: "SEG-01", from_s: 0, to_s: 150 },
      { segment: "SEG-02A", from_s: 150, to_s: 300 },
    ],
    events,
    tags: {
      session_id: id,
      tagger_version: 1,
      model: "haiku",
      created_ms: 1_792_500_000_000,
      lines: events.flatMap((e, i) => (e.type === "say" ? [{ line: i + 1, kind: "roast" as const, target: "viewer" as const, edge: "sharp" as const, why: long(160, "why") }] : [])),
      untagged: 0,
    },
    verdict: bigVerdict(id),
    catalog: [],
    calls: [],
    feedback: feedback(id),
  };
}

const RUN: RunRow = {
  run_id: "theatrical.haiku.SEG-01.a30.s1",
  tournament_id: "pilot-d1ef98d3a6",
  has_verdict: true,
  red_line_fails: 2,
  health: 0.93,
  spec: { run_id: "theatrical.haiku.SEG-01.a30.s1", audience_size: 30, character: "malakor", core_effort: "low", core_tier: "haiku", duration_s: 600, population: "growing_channel", seed: 1, segment: "SEG-01", tone_variant: "theatrical" },
};
const LEDGER: LedgerEntry[] = ledgerRows(RUN, 3000);

const ROLES = ["core_thread", "chat_picker", "planner", "researcher", "consolidator", "chatter", "judge", "reconciler", "clipper", "tagger", "mod_classifier", "mod_voice"] as const;
export function engineStatus(mode: EngineStatus["fleet"]["mode"] = "scripted"): EngineStatus {
  return {
    accounts: [{ label: "main" }, { label: "spare", cooling_until_ms: 1_800_000_000_000 }, { label: "third" }],
    api_fallback: true,
    fallbacks: ["codex", "agy", "local_gemma"],
    fleet: { mode, since_ms: 0 },
    routes: ROLES.map((role) => ({ role, tier: role === "judge" ? "opus" : "haiku", effort: "low", overridden: role === "core_thread", timeout_ms: 120_000 })),
  };
}

const running: SandboxStatus = { session_id: BIG_SESSION, state: "running", core_errors: 0, elapsed_s: 120, events: 80, max_say_gap_s: 4, planned_s: 300 };
const tournaments: TournamentRow[] = Array.from({ length: 60 }, (_, i) => ({
  tournament_id: `pilot-${String(i).padStart(10, "0")}`,
  character: "malakor",
  instrument_hash: "sha256:4b1d",
  runs: 72,
  completed: 72,
  judged: 72,
  held: 0.91,
  red_line_fails: i % 4,
  updated_ms: 1_792_500_000_000 + i * 1000,
}));

/** `limit` items from a paged list, with a cursor while more remain. */
function page<T>(all: readonly T[], params: unknown): { items: T[]; next_cursor?: string } {
  const p = (params ?? {}) as { limit?: number; cursor?: string };
  const from = Number(p.cursor ?? 0);
  const items = all.slice(from, from + (p.limit ?? 50));
  return from + items.length < all.length ? { items, next_cursor: String(from + items.length) } : { items };
}

const PACKAGE_FILES = [
  "character.toml", "constitution.md", "bible.md", "directions.md", "stalls.toml", "situations.json",
  ...["deadpan-bureaucrat", "gleeful-chaos", "theatrical", "unhinged-roastmaster", "weary-middle-manager"].flatMap((t) => [`tones/${t}.md`, `lines/${t}.toml`]),
  ...["the-coin-prophet", "the-disruptor", "the-has-been-hero", "the-inspector", "the-life-coach"].map((f) => `foils/${f}.md`),
  "rules/crisis-handoff.md", "rules/no-financial-promotion.md",
];

type Answers = { readonly [M in RpcMethod]?: (params: unknown) => RpcResult<M> };

const ANSWERS: Answers = {
  "review.sessions": () => sessionRows(40).map((r, i) => ({ ...r, session_id: `sbx-20261020-${String(221153 + i)}-30m`, rule_fails: 3 })),
  "review.session": (p) => bigSessionOf((p as { session_id: string }).session_id),
  "review.feedback.set": (p) => ({ ...(p as Omit<LineFeedback, "at_ms">), at_ms: 1_792_500_200_000 }),
  "run.transcript": () => bigEvents(BIG_RUN),
  "run.verdict": () => bigVerdict(BIG_RUN),
  "runs.page": (p) => page(Array.from({ length: 400 }, (_, i) => ({ ...RUN, run_id: `${RUN.run_id}.${i}` })), p),
  "tournament.list": () => tournaments,
  "tournament.results": () => genResults(36, 3),
  "ranks.table": () => genRanks(),
  "ledger.summary": () => ledgerSummary(LEDGER),
  "ledger.page": (p) => page(LEDGER, p),
  "engine.status": () => engineStatus(),
  "sandbox.status": () => running,
  "sandbox.start": () => ({ session_id: BIG_SESSION, dir: "runs/sandbox/sbx-20261020-221153-5m", stage_url: "http://localhost:7717/stage/?session=sbx-20261020-221153-5m" }),
  "sandbox.stop": () => ({ ...running, state: "closing" }),
  "character.list": () => genRoster(12).map((c) => ({ ...c, tagline: long(120, "tagline") })),
  "character.get": () => ({ ...malakorParams(), files: PACKAGE_FILES.map((path, i) => ({ path, bytes: 400 + i * 97 })) }),
  "character.doc.get": (p) => {
    const { id, doc } = p as { id: string; doc: string };
    const d = malakorParams();
    const text = doc === "bible.md" ? (d.bible ?? "") : doc === "constitution.md" ? d.constitution : long(6000, doc);
    return { id, doc, text, digest: `sha256:${"c3a1".repeat(16)}` };
  },
  "character.doc.set": (p) => ({ id: (p as { id: string }).id, doc: (p as { doc: string }).doc, digest: `sha256:${"9f0e".repeat(16)}`, previous_digest: `sha256:${"c3a1".repeat(16)}`, backup: "character-backups/malakor/bible.md.1792500300000" }),
  "character.validate": () => Array.from({ length: 40 }, (_, k) => ({ file: `situations.json`, key: `situations[${k}].required`, reason: long(140, "reason"), severity: k % 5 === 0 ? ("error" as const) : ("warning" as const) })),
  "character.rules.set": () => malakorRules(),
  "character.rules": () => malakorRules(),
  "character.tone.set": () => ({ id: "malakor", name: "Arch-Lich Malakor", active_tone: "theatrical", default_tone: "theatrical", tones: [], foils: [], rules_enabled: 2, rules_total: 2 }),
  "tone.lines.add": (p) => ({ file: "lines/theatrical.toml", line: { id: "l-0042", status: "approved", tags: ["roast_request"], text: (p as { text: string }).text } }),
  "tone.lines.set_status": () => ({ file: "lines/theatrical.toml", line: { id: "l-0042", status: "retired", tags: ["roast_request"], text: long(300, "line") } }),
  "segments.tables": () => genTables(),
  "reconcile.report": () => genReport(),
  "sim.start": (p) => ({ run_id: (p as { spec: { run_id: string } }).spec.run_id }),
};

/** The realistic daemon as a testkit Script. */
export function worldScript(): Script {
  const script: Record<string, (params: unknown) => Reply> = {};
  for (const [m, fn] of Object.entries(ANSWERS)) script[m] = (params) => ok((fn as (p: unknown) => unknown)(params));
  return script;
}
