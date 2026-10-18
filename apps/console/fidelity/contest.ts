// The contest's real data (`shadowrunr-data.js`, the file both prototypes loaded) mapped to the
// daemon's RPC shapes, for the fidelity harness only. Every reply still passes the contract
// door, so a shape this mapping gets wrong fails loudly in the harness instead of rendering.
//
// Where the contest data holds the thing itself (the pilot tournament, its runs, transcripts,
// verdicts and ledgers, the Malakor package, the segment and rank TOML, the Reconcile report and
// its proposals) it is served as is. Where it holds nothing (the daemon's compiled-in command
// table and op tags) the segments page's own copy of the daemon table is used, and the plan,
// directives and engine status answer as a fresh daemon would.
import type {
  CharacterDetail,
  CharacterSummary,
  Rank,
  RankInfo,
  RankTableInfo,
  ReportSummary,
  RunRow,
  SegmentId,
  SegmentStatus,
  SegmentTableInfo,
  SegmentTables,
  Situation,
  TournamentRow,
} from "@shadowrunr/contract";
import { COMMAND_INFO } from "../src/pages/segments/testgen";
import type { FetchReply } from "../src/data/testing";
import { parseToml, type TomlTable, type TomlValue } from "./toml";

/** The parts of `window.SHADOWRUNR_DATA` the harness reads (the door checks the rest). */
export interface ContestData {
  readonly pilot: {
    readonly results: { readonly tournament_id: string; readonly instrument_hashes: readonly string[] } & Record<string, unknown>;
    readonly runs: readonly ContestRun[];
  };
  readonly character: Readonly<Record<string, unknown>>;
  readonly segments: Readonly<Record<string, unknown>>;
  readonly reconcile: {
    readonly report: { readonly report_id: string; readonly created_ms: number; readonly source: ReportSummary["source"] } & Record<string, unknown>;
    readonly proposals: readonly unknown[];
  };
}

interface ContestRun {
  readonly run_id: string;
  readonly spec: RunRow["spec"];
  readonly verdict?: unknown;
  readonly transcript: readonly unknown[];
  readonly ledger: readonly ({ readonly ts_unix_ms: number } & Record<string, unknown>)[];
}

const SEGMENT_IDS: readonly SegmentId[] = ["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II"];
const STATUSES: readonly SegmentStatus[] = ["pilot", "trial", "regular", "flagship", "retired", "descoped"];
const RANKS: readonly Rank[] = ["thrall", "acolyte", "cultist", "warlock", "lich", "dread_council"];
/** The daemon's op tags (crates/shadowrunr-types Op); the contest data does not list them. */
const OP_TAGS = ["highlight", "start_poll", "close_poll", "summon_foil", "dismiss_foil", "reveal_hex", "dispatch_research", "show_card", "mark_moment", "switch_segment", "sign_off"];

const text = (files: Readonly<Record<string, unknown>>, path: string): string => {
  const v = files[path];
  if (typeof v !== "string") throw new Error(`contest data: ${path} is missing`);
  return v;
};
const str = (t: TomlTable, k: string): string => {
  const v = t[k];
  if (typeof v !== "string") throw new Error(`toml: ${k} is not a string`);
  return v;
};
const num = (t: TomlTable, k: string, fallback?: number): number => {
  const v = t[k] ?? fallback;
  if (typeof v !== "number") throw new Error(`toml: ${k} is not a number`);
  return v;
};
/** The daemon's serde defaults for an omitted clock rule (crates/shadowrunr-segments table.rs). */
const CLOCK_DEFAULTS = { foil_vote_secs: 60, poll_min_secs: 10, poll_max_secs: 600, max_poll_options: 6 } as const;
const strs = (v: TomlValue | undefined): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
const tables = (v: TomlValue | undefined): TomlTable[] => (Array.isArray(v) ? v.filter((x): x is TomlTable => typeof x === "object" && !Array.isArray(x)) : []);
const sub = (t: TomlTable, k: string): TomlTable | undefined => {
  const v = t[k];
  return typeof v === "object" && !Array.isArray(v) ? v : undefined;
};
const segmentId = (s: string): SegmentId => {
  const id = SEGMENT_IDS.find((x) => x === s);
  if (id === undefined) throw new Error(`unknown segment ${s}`);
  return id;
};
const rank = (s: string): Rank => {
  const r = RANKS.find((x) => x === s);
  if (r === undefined) throw new Error(`unknown rank ${s}`);
  return r;
};
const under = (files: Readonly<Record<string, unknown>>, dir: string) =>
  Object.keys(files)
    .filter((k) => k.startsWith(`${dir}/`))
    .sort()
    .map((k) => ({ id: k.slice(dir.length + 1).replace(/\.md$/, ""), text: text(files, k) }));

/** situations.json is served as parsed JSON; the door validates each situation. */
const isSituations = (v: unknown): v is { situations: Situation[] } =>
  typeof v === "object" && v !== null && "situations" in v && Array.isArray(v.situations);

function character(d: ContestData): { summary: CharacterSummary; detail: CharacterDetail } {
  const files = d.character;
  const toml = parseToml(text(files, "character.toml"));
  const titles = sub(toml, "ranks") ?? {};
  const tones = under(files, "tones");
  const foilFiles = under(files, "foils");
  const declared = strs(toml["foils"]);
  const situations = files["situations.json"];
  const id = str(toml, "id");
  const name = str(toml, "name");
  const defaultTone = str(toml, "default_tone");
  const summary: CharacterSummary = {
    id,
    name,
    default_tone: defaultTone,
    active_tone: defaultTone,
    foils: declared,
    tones: tones.map((t) => t.id),
    rules_enabled: 0,
    rules_total: 0,
  };
  const detail: CharacterDetail = {
    id,
    name,
    default_tone: defaultTone,
    active_tone: defaultTone,
    constitution: text(files, "constitution.md"),
    bible: text(files, "bible.md"),
    files: Object.keys(files)
      .sort()
      .map((path) => ({ path, bytes: new TextEncoder().encode(typeof files[path] === "string" ? String(files[path]) : JSON.stringify(files[path])).length })),
    foils: foilFiles.map((f) => ({ id: f.id, declared: declared.includes(f.id), text: f.text })),
    rank_titles: Object.fromEntries(RANKS.filter((r) => typeof titles[r] === "string").map((r) => [r, str(titles, r)])),
    rules: [],
    situations: isSituations(situations) ? situations.situations : [],
    tones,
  };
  return { summary, detail };
}

function segmentTable(src: string): SegmentTableInfo {
  const t = parseToml(src);
  const clock = sub(t, "clock") ?? {};
  const handover = sub(clock, "handover");
  const status = STATUSES.find((s) => s === str(t, "status"));
  if (status === undefined) throw new Error(`unknown status ${str(t, "status")}`);
  return {
    id: segmentId(str(t, "id")),
    title: str(t, "title"),
    status,
    initial: str(t, "initial"),
    states: strs(t["states"]),
    allowed_ops: strs(t["allowed_ops"]),
    commands: strs(t["commands"]),
    switch_to: strs(t["switch_to"]).map(segmentId),
    rules_prompt: typeof t["rules_prompt"] === "string" ? t["rules_prompt"] : "",
    ...(typeof t["design_notes"] === "string" ? { design_notes: t["design_notes"] } : {}),
    clock: {
      foil_vote_secs: num(clock, "foil_vote_secs", CLOCK_DEFAULTS.foil_vote_secs),
      poll_min_secs: num(clock, "poll_min_secs", CLOCK_DEFAULTS.poll_min_secs),
      poll_max_secs: num(clock, "poll_max_secs", CLOCK_DEFAULTS.poll_max_secs),
      max_poll_options: num(clock, "max_poll_options", CLOCK_DEFAULTS.max_poll_options),
      ...(handover
        ? { handover: { to: segmentId(str(handover, "to")), ccv_min: num(handover, "ccv_min"), sustain_s: num(handover, "sustain_s"), max_elapsed_s: num(handover, "max_elapsed_s"), grace_s: num(handover, "grace_s") } }
        : {}),
    },
    transitions: tables(t["transitions"]).map((r) => ({ from: str(r, "from"), on: str(r, "on"), ...(typeof r["guard"] === "string" ? { guard: r["guard"] } : {}), to: str(r, "to") })),
  };
}

function segmentTables(d: ContestData): SegmentTables {
  const segments = Object.keys(d.segments)
    .filter((k) => k.startsWith("segments/"))
    .sort()
    .map((k) => segmentTable(text(d.segments, k)));
  return { segments, commands: COMMAND_INFO, command_names: COMMAND_INFO.map((c) => c.name), op_tags: OP_TAGS };
}

function rankTable(d: ContestData, titles: CharacterDetail["rank_titles"], characterId: string): RankTableInfo {
  const t = parseToml(text(d.segments, "ranks.toml"));
  const ranks: RankInfo[] = tables(t["ranks"]).map((r) => {
    const id = rank(str(r, "rank"));
    const tiers = tables(r["tiers"]).map((x) => ({ tier: num(x, "tier"), min_months: num(x, "min_months") }));
    return {
      rank: id,
      title: titles[id] ?? id,
      weight: num(r, "weight"),
      ...(typeof r["min_gifted"] === "number" ? { min_gifted: r["min_gifted"] } : {}),
      ...(tiers.length > 0 ? { tiers } : {}),
    };
  });
  return { character: characterId, propose_min_rank: rank(str(t, "propose_min_rank")), ranks };
}

/** The daemon's `verdict_health` (shadowrunr-daemon runs.rs): held share of judged checks, red-line fails. */
function verdictHealth(verdict: unknown): Pick<RunRow, "health" | "red_line_fails"> {
  const rules = typeof verdict === "object" && verdict !== null && "rules" in verdict && Array.isArray(verdict.rules) ? (verdict.rules as readonly { rule?: unknown; state?: unknown }[]) : [];
  const pass = rules.filter((r) => r.state === "pass").length;
  const failed = rules.filter((r) => r.state === "fail");
  const red = failed.filter((r) => typeof r.rule === "string" && r.rule.startsWith("red-line")).length;
  const n = pass + failed.length;
  return { ...(n > 0 ? { health: pass / n } : {}), ...(red > 0 ? { red_line_fails: red } : {}) };
}

const ok = (result: unknown): FetchReply => ({ body: { result } });
const fail = (code: string, message: string): FetchReply => ({ status: 400, body: { error: { code, message } } });

/** A request body's params (the harness's own requests: plain JSON from the RPC client). */
type Params = Readonly<Record<string, unknown>>;

function pageOf<T>(items: readonly T[], p: Params): FetchReply {
  const start = typeof p["cursor"] === "string" ? Number(p["cursor"]) : 0;
  const limit = typeof p["limit"] === "number" ? Math.min(500, p["limit"]) : 500;
  const end = start + limit;
  return ok({ items: items.slice(start, end), ...(end < items.length ? { next_cursor: String(end) } : {}) });
}

/** Answers one RPC from the contest data, by method. */
export function contestRpc(d: ContestData): (method: string, params: Params) => FetchReply {
  const { summary, detail } = character(d);
  const tables = segmentTables(d);
  const ranks = rankTable(d, detail.rank_titles, summary.id);
  const tid = d.pilot.results.tournament_id;
  const runs: RunRow[] = d.pilot.runs.map((r) => ({ run_id: r.run_id, tournament_id: tid, has_verdict: r.verdict !== undefined, spec: r.spec, ...verdictHealth(r.verdict) }));
  const tournament: TournamentRow = { tournament_id: tid, instrument_hash: d.pilot.results.instrument_hashes[0] ?? "", runs: runs.length, completed: runs.length };
  // The ledger file's rows, as `ledger.page` serves them: `ts_ms` for the file's `ts_unix_ms`,
  // the run and tournament ids added, and the file-only `session` flag dropped.
  const ledger = d.pilot.runs.flatMap((r) =>
    r.ledger.map((row) => {
      const { ts_unix_ms, session: _session, ...rest } = row;
      return { runId: r.run_id, entry: { ...rest, ts_ms: ts_unix_ms, run_id: r.run_id, tournament_id: tid } };
    }),
  );
  const report = d.reconcile.report;
  const reports: ReportSummary[] = [{ report_id: report.report_id, created_ms: report.created_ms, source: report.source, proposals: d.reconcile.proposals.length }];
  const run = (p: Params) => d.pilot.runs.find((r) => r.run_id === p["run_id"]);

  return (method, p) => {
    switch (method) {
      case "character.list":
        return ok([summary]);
      case "character.get":
        return p["id"] === summary.id ? ok(detail) : fail("not_found", "no such character");
      case "character.rules":
        return ok([]);
      case "segments.tables":
        return ok(tables);
      case "ranks.table":
        return ok(ranks);
      case "reconcile.list":
        return ok(reports);
      case "reconcile.report":
        return p["report_id"] === report.report_id ? ok(report) : fail("not_found", "no such report");
      case "plan.proposals":
        return ok(d.reconcile.proposals);
      case "plan.current":
        return fail("not_found", "no plan has been chosen yet");
      case "directive.list":
        return ok([]);
      case "tournament.list":
        return ok([tournament]);
      case "tournaments.page":
        return pageOf([tournament], p);
      case "tournament.results":
        return p["tournament_id"] === tid ? ok(d.pilot.results) : fail("not_found", "no such tournament");
      case "runs.page": {
        const filter = (p["filter"] ?? {}) as Params;
        const rows = runs.filter((r) => (p["tournament_id"] === undefined || r.tournament_id === p["tournament_id"]) && (filter["has_verdict"] === undefined || r.has_verdict === filter["has_verdict"]));
        return pageOf(rows, p);
      }
      case "run.transcript": {
        const r = run(p);
        return r ? ok(r.transcript) : fail("not_found", "no such run");
      }
      case "run.verdict": {
        const r = run(p);
        return r?.verdict !== undefined ? ok(r.verdict) : fail("not_found", "no verdict");
      }
      case "ledger.page":
        return pageOf(
          ledger.filter((e) => p["run_id"] === undefined || e.runId === p["run_id"]).map((e) => e.entry),
          p,
        );
      default:
        return fail("unavailable", `the fidelity harness does not serve ${method}`);
    }
  };
}

/**
 * Live `/events` frames for a named scene. `seg02a`: the Hexmap Investigation opening on the
 * contest's demo case (the pilot's stored runs are all SEG-01, so no stored run reaches it).
 */
export function liveScene(d: ContestData, scene: string): readonly unknown[] {
  if (scene !== "seg02a") throw new Error(`unknown harness scene ${scene}`);
  const run = "fidelity-live";
  const events: unknown[] = [
    { type: "stream_start", ts_ms: 0, stream_id: run, character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 30 },
    { type: "segment_start", ts_ms: 0, segment: "SEG-02A" },
    { type: "case_open", ts_ms: 10, case: d.segments["demo-case.json"] },
  ];
  return events.map((event, i) => ({ seq: i + 1, run_id: run, event }));
}
