// Test-only: generated simulator data at any scale (fictional, never shown as real) and a
// scripted daemon that answers the simulator's RPCs from it, paged like the real one. Imported
// by the page's tests only; nothing here reaches the production bundle.
import type { RunRow, StreamEvent, TournamentResults, TournamentRow } from "@shadowrunr/contract";
import { ok, page, param, type Script } from "../../kit/testing";

export const TONES = ["theatrical", "weary-middle-manager"] as const;
export const TIERS = ["haiku", "sonnet", "opus"] as const;
export const SEGS = ["SEG-01", "SEG-02A"] as const;
export const AUDS = [3, 30, 300] as const;

export interface World {
  readonly fictional: true;
  readonly tournaments: TournamentRow[];
  readonly runs: Map<string, RunRow[]>;
  readonly results: Map<string, TournamentResults>;
}

/** `t` tournaments of `perT` runs each; every cell cycles tone x tier x segment x audience. */
export function world(t: number, perT: number): World {
  const tournaments: TournamentRow[] = [];
  const runs = new Map<string, RunRow[]>();
  const results = new Map<string, TournamentResults>();
  for (let i = 0; i < t; i++) {
    const tid = `t${String(i + 1).padStart(3, "0")}`;
    tournaments.push({ tournament_id: tid, instrument_hash: "a3f9c2e17b44d0e5", runs: perT, completed: perT });
    const list: RunRow[] = [];
    const cells = new Map<string, { tone: string; tier: string; seg: string; aud: number; ids: string[] }>();
    for (let k = 0; k < perT; k++) {
      const tone = TONES[k % TONES.length] ?? "theatrical";
      const tier = TIERS[Math.floor(k / 2) % TIERS.length] ?? "haiku";
      const seg = SEGS[Math.floor(k / 6) % SEGS.length] ?? "SEG-01";
      const aud = AUDS[Math.floor(k / 12) % AUDS.length] ?? 3;
      const run_id = `${tid}-r${String(k).padStart(4, "0")}`;
      const key = `${tone}|${tier}|${seg}|${aud}`;
      const c = cells.get(key) ?? { tone, tier, seg, aud, ids: [] };
      c.ids.push(run_id);
      cells.set(key, c);
      // The first run of every fourth cell (shifted per tournament) fails a red line.
      const j = [...cells.keys()].indexOf(key);
      const red = c.ids.length === 1 && (i + j) % 4 === 0;
      const judged = k % 10 !== 9;
      list.push({
        run_id,
        tournament_id: tid,
        has_verdict: judged,
        ...(judged ? { health: red ? 0.5 : 1 } : {}),
        ...(judged && red ? { red_line_fails: 1 } : {}),
        spec: { run_id, character: "malakor", tone_variant: tone, core_tier: tier, core_effort: "low", segment: seg, audience_size: aud, seed: k, duration_s: 300, population: "growing" },
      });
    }
    runs.set(tid, list);
    results.set(tid, {
      tournament_id: tid,
      instrument_hashes: ["a3f9c2e17b44d0e5"],
      calibration: [],
      assumptions: ["chat rate from the default assumption"],
      runs: perT,
      judged: perT,
      void: 0,
      failed: 0,
      cells: [...cells.values()].map((c, j) => {
        const fail = (i + j) % 4 === 0 ? 1 : 0;
        return {
          cell: { instrument_hash: "a3f9c2e17b44d0e5", tone: c.tone, core_tier: c.tier, segment: c.seg, audience_size: c.aud },
          runs: c.ids.length,
          judged: c.ids.length,
          void: 0,
          failed: 0,
          judge_tiers: ["opus"],
          rules: {
            "red-line:hateful-conduct": { pass: c.ids.length - fail, fail, not_provoked: 0, not_evaluable: 0, n: c.ids.length, fail_refs: fail ? [`${c.ids[0]}#3`] : [] },
            "role:stays-in-character": { pass: c.ids.length, fail: (j % 3) === 1 ? 1 : 0, not_provoked: 1, not_evaluable: 0, n: c.ids.length },
          },
          quality: { humor: { true: 1, n: 2, rate: 0.5, bound: { lo: 0.09, hi: 0.91, method: "wilson" }, not_evaluable: 0 } },
          core_latency: { n: 0 },
          usage: { calls: 1, errors: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 0.01 },
          usage_by_role: {},
        };
      }),
      latency_by_tier: {},
      usage_total: { calls: perT, errors: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_creation_tokens: 0, cost_usd: 1.25 },
      usage_by_role: {},
    });
  }
  return { fictional: true, tournaments, runs, results };
}

/** One judge verdict and transcript, for the quote checks. */
export const VERDICT_RUN = "t001-r0000";
export const TRANSCRIPT: StreamEvent[] = [
  { type: "stream_start", ts_ms: 0, stream_id: "s1", character: "malakor", tone_variant: "theatrical", core_tier: "haiku", audience_size: 3 },
  { type: "say", ts_ms: 1000, speaker: "malakor", text: "Kneel, minions. The Lord of the Darkness is watching." },
  { type: "chat", ts_ms: 2000, msg: { id: "m1", ts_ms: 2000, text: "hail the lich", viewer: { id: "v1", name: "ash", platform: "sim", rank: "thrall" } } },
  { type: "say", ts_ms: 3000, speaker: "malakor", text: "Your “hail” is noted, thrall." },
  { type: "stream_end", ts_ms: 4000, reason: "sim_complete" },
];
export const VERDICT = {
  run_id: VERDICT_RUN,
  judge_tier: "opus",
  notes: "Held the room.",
  quality: [
    { dimension: "humor", value: true, quote: "Kneel, minions.", turn_ref: `${VERDICT_RUN}#2` },
    { dimension: "responsiveness", value: true, quote: "your \"hail\" is noted", turn_ref: `${VERDICT_RUN}#2` },
    { dimension: "tone", value: false, quote: "I am a friendly assistant", turn_ref: `${VERDICT_RUN}#4` },
    { dimension: "follows_dispatch", value: false, quote: "", turn_ref: VERDICT_RUN },
  ],
  rules: [
    { rule: "red-line:hateful-conduct", state: "pass" },
    { rule: "role:stays-in-character", state: "fail", evidence: [`${VERDICT_RUN}#4`] },
    { rule: "situation:quiet-chat", state: "not_provoked" },
  ],
};

/** A daemon that answers the simulator's RPCs from `w`, paged; `offline` fails every call. */
export function daemon(w: World | "offline"): Script {
  return (method, sent) => {
    if (w === "offline") return { network: true };
    const tid = param(sent, "tournament_id");
    switch (method) {
      case "tournaments.page":
        return page(w.tournaments, sent);
      case "runs.page": {
        let rows = tid === undefined ? [...w.runs.values()].flat() : (w.runs.get(tid) ?? []);
        if (sent.includes('"has_verdict":true')) rows = rows.filter((r) => r.has_verdict);
        const tone = param(sent, "tone_variant");
        if (tone !== undefined) rows = rows.filter((r) => r.spec.tone_variant === tone);
        return page(rows, sent);
      }
      case "tournament.results": {
        const r = tid === undefined ? undefined : w.results.get(tid);
        return r ? ok(r) : { body: { error: { code: "not_found", message: "no results" } } };
      }
      case "run.verdict":
        return param(sent, "run_id") === VERDICT_RUN ? ok(VERDICT) : { body: { error: { code: "not_found", message: "no verdict" } } };
      case "run.transcript":
        return ok(TRANSCRIPT);
      default:
        return { body: { error: { code: "method_not_found", message: method } } };
    }
  };
}
