// A `review.session` reply for the layer 2 tab tests, shaped exactly like the daemon's (it passes
// the contract door). Lines repeat in blocks of five: a chat message, Malakor answering it, a
// moderation record, the foil, Malakor again. Imported by tests only.
import type { LedgerEntry, LineFeedback, LineKind, LineTag, ReviewSession, Role, StreamEvent } from "@shadowrunr/contract";

export const SESSION = "sbx-20261020-120000-30m";
export const STARTED_MS = 1_792_510_277_256;
/** Stream milliseconds per transcript line. */
const STEP_MS = 500;

const KINDS: readonly LineKind[] = ["roast", "tease", "joke", "show", "callout"];
const MODS: readonly (readonly ["input" | "output", string, string])[] = [
  ["input", "rate_cap", "dropped"],
  ["output", "persona_leak", "replaced"],
  ["input", "flood_cluster", "collapsed:3"],
  ["output", "engine_stall", "covered"],
  ["output", "alleged:case-subject", "hedged"],
];
const ROLES: readonly Role[] = ["core_thread", "chatter", "chat_picker", "judge"];

/** The tag kind of the n-th Malakor line (0-based); every 13th is left untagged. */
export const kindOf = (n: number): LineKind | undefined => (n % 13 === 12 ? undefined : KINDS[n % KINDS.length]);

export function reviewSession(opts: { events?: number; marked?: readonly number[]; judged?: boolean; calls?: boolean } = {}): ReviewSession {
  const total = opts.events ?? 60;
  const events: StreamEvent[] = [
    { type: "stream_start", ts_ms: 0, stream_id: SESSION, character: "malakor", tone_variant: "unhinged-roastmaster", core_tier: "sonnet", audience_size: 30 },
    { type: "segment_start", ts_ms: 0, segment: "SEG-01" },
  ];
  const tags: LineTag[] = [];
  let says = 0;
  for (let line = 3; line <= total; line++) {
    const ts_ms = line * STEP_MS;
    const k = (line - 3) % 5;
    if (k === 0) {
      events.push({ type: "chat", ts_ms, msg: { id: `m${line}`, text: `chat message ${line}`, ts_ms, viewer: { id: `v${line % 9}`, name: `Viewer${line % 9}`, platform: "sim", rank: "acolyte" } } });
    } else if (k === 2) {
      const [stage, rule, action] = MODS[Math.floor(line / 5) % MODS.length] ?? MODS[0] ?? ["input", "rate_cap", "dropped"];
      events.push({ type: "moderation", ts_ms, stage, rule, action });
    } else if (k === 3) {
      events.push({ type: "say", ts_ms, speaker: "foil", foil: "fake-detective", text: `The foil interrupts at ${line}.` });
    } else {
      events.push({ type: "say", ts_ms, speaker: "host", text: `Malakor speaks line ${line}.`, ...(k === 1 ? { replying_to: [`m${line - 1}`] } : {}) });
      const kind = kindOf(says++);
      if (kind) tags.push({ line, kind, target: "viewer", edge: line % 2 ? "mild" : "sharp", why: "names a viewer" });
    }
  }
  const lengthS = (total * STEP_MS) / 1000;
  const sayLines = events.flatMap((e, i) => (e.type === "say" && e.speaker === "host" ? [i + 1] : []));
  const feedback: LineFeedback[] = (opts.marked ?? []).map((line, i) => ({ session_id: SESSION, line, tags: [i % 2 ? "flat" : "landed"], at_ms: STARTED_MS + i }));
  const mix: Record<string, number> = {};
  for (const t of tags) mix[t.kind] = (mix[t.kind] ?? 0) + 1;
  const calls: LedgerEntry[] =
    opts.calls === false
      ? []
      : Array.from({ length: Math.floor(lengthS / 2) }, (_, i) => ({
          ts_ms: STARTED_MS + i * 2000,
          role: ROLES[i % ROLES.length] ?? "core_thread",
          tier: "sonnet",
          model: "claude-sonnet-5-5",
          duration_ms: 1000 + (i % 7) * 500,
          usage: { input_tokens: 10, output_tokens: 40 },
          ...(i % ROLES.length === 0 ? { first_text_ms: 800 + (i % 5) * 100 } : {}),
          cost_usd: 0.01,
          ...(i % 17 === 16 ? { error_kind: "timeout" } : {}),
        }));
  const ref = (line: number) => `${SESSION}#${line}`;
  return {
    row: {
      session_id: SESSION,
      minutes: 30,
      started_ms: STARTED_MS,
      length_s: lengthS,
      pass: false,
      checks: [
        { id: "say_gap", state: "pass", detail: "max 2.0 s" },
        { id: "judge", state: "fail", detail: "2 rules failed" },
      ],
      max_say_gap_s: 2,
      ...(opts.judged === false ? {} : { rule_fails: 2 }),
      lines: sayLines.length,
      mix,
      marked: feedback.length,
      tagged: true,
    },
    segments: [{ segment: "SEG-01", from_s: 0, to_s: lengthS }],
    events,
    tags: { session_id: SESSION, tagger_version: 1, model: "haiku", created_ms: STARTED_MS, lines: tags, untagged: sayLines.length - tags.length },
    ...(opts.judged === false
      ? {}
      : {
          verdict: {
            run_id: SESSION,
            judge_tier: "opus",
            notes: "He kept the room moving but lectured once.",
            rules: [
              // Line 3 is a chat line: its evidence opens the next Malakor line, 4.
              { rule: "situation:edgy-roast-request", state: "fail", evidence: [ref(3), ref(7)] },
              { rule: "red-line:no-lecture", state: "fail", evidence: [ref(12)] },
              { rule: "situation:empty-room", state: "pass" },
            ],
            quality: sayLines.slice(0, 6).flatMap((line, i) => [
              { dimension: "humor_landed", value: i % 3 === 0, quote: "Malakor", turn_ref: ref(line) },
              { dimension: "in_character", value: true, quote: "Malakor", turn_ref: ref(line) },
            ]),
          },
        }),
    catalog: [
      { id: "situation:edgy-roast-request", when: "a viewer asks to be roasted hard", required: "roast them in character, without cruelty" },
      { id: "red-line:no-lecture", required: "Never lecture chat." },
      { id: "q:humor_landed", required: "Did the joke land with chat?" },
    ],
    calls,
    feedback,
  };
}

/** Malakor lines of one kind in a fixture, in transcript order. */
export function linesOf(rs: ReviewSession, kind: LineKind): number[] {
  return (rs.tags?.lines ?? []).filter((t) => t.kind === kind).map((t) => t.line);
}
