// Event-driven cache invalidation: which cached RPC replies an event or a mutation makes stale.
// A rule names a method and, optionally, a scope over the cached entry's params; an entry the
// scope cannot prove irrelevant is invalidated, so a scope only ever skips refetches.
// Polling is only the slow backstop in `QueryCache`.
import type { RpcMethod, StreamEvent } from "@shadowrunr/contract";

/** A cached reply as a scope sees it. `fresh`: answered, not stale, no refetch in flight. */
export interface CachedReply {
  readonly params: unknown;
  readonly data: unknown;
  readonly fresh: boolean;
}

export interface ScopeContext {
  /** The run an event belongs to (its envelope's run id), when it says. */
  readonly runId?: string;
  /** A mutation's params. */
  readonly params?: unknown;
  /** The cached replies of one method. */
  readonly cached: (method: RpcMethod) => Iterable<CachedReply>;
}

/** False only when the entry with these params provably did not change. */
export type Scope = (entryParams: unknown, ctx: ScopeContext) => boolean;

export type Rule = RpcMethod | { readonly method: RpcMethod; readonly scope: Scope };

type Fields = Readonly<Record<string, unknown>>;
const fields = (v: unknown): Fields => (typeof v === "object" && v !== null ? (v as Fields) : {});

interface RunLike {
  readonly run_id?: unknown;
  readonly tournament_id?: unknown;
}

/**
 * Whether cached run listings tie `runId` to tournament `tid`: true when a row says so, false
 * when a fresh, complete, unfiltered listing covering `tid` lacks the run, undefined otherwise.
 */
export function runInTournament(runId: string, tid: string, cached: ScopeContext["cached"]): boolean | undefined {
  const covers = (p: Fields) => p.tournament_id === undefined || p.tournament_id === tid;
  let complete = false;
  for (const c of cached("runs.list")) {
    const rows = Array.isArray(c.data) ? (c.data as RunLike[]) : [];
    if (rows.some((r) => r.run_id === runId && r.tournament_id === tid)) return true;
    if (c.fresh && covers(fields(c.params))) complete = true;
  }
  // runs.page: a chain is complete when its first page leads, page by page, to one with no cursor.
  const chains = new Map<string, Map<string, CachedReply>>();
  for (const c of cached("runs.page")) {
    const page = fields(c.data);
    const items = Array.isArray(page.items) ? (page.items as RunLike[]) : [];
    if (items.some((r) => r.run_id === runId && r.tournament_id === tid)) return true;
    const p = fields(c.params);
    if (!c.fresh || p.filter !== undefined || !covers(p)) continue;
    const id = `${String(p.tournament_id)} ${String(p.limit)}`;
    const chain = chains.get(id) ?? new Map<string, CachedReply>();
    chains.set(id, chain.set(typeof p.cursor === "string" ? p.cursor : "", c));
  }
  for (const chain of chains.values()) {
    let page = chain.get("");
    for (let n = 0; page !== undefined && n <= chain.size; n += 1) {
      const next = fields(page.data).next_cursor;
      if (typeof next !== "string") complete = true;
      page = typeof next === "string" ? chain.get(next) : undefined;
    }
  }
  return complete ? false : undefined;
}

/** A ledger read changed with the event's run: its run, a tournament holding it, or the engine ledger. */
const byRun: Scope = (entryParams, ctx) => {
  const p = fields(entryParams);
  const runId = ctx.runId;
  if (runId === undefined) return true;
  if (typeof p.run_id === "string") return p.run_id === runId;
  if (typeof p.tournament_id === "string") return runInTournament(runId, p.tournament_id, ctx.cached) !== false;
  return true;
};

/** The entry's `entryKey` names the record the mutation's `paramKey` names (or is unkeyed). */
const sameId =
  (paramKey: string, entryKey = "id"): Scope =>
  (entryParams, ctx) => {
    const want = fields(ctx.params)[paramKey];
    const have = fields(entryParams)[entryKey];
    return typeof want !== "string" || typeof have !== "string" || want === have;
  };

const ledger: readonly Rule[] = [
  { method: "ledger.page", scope: byRun },
  { method: "ledger.summary", scope: byRun },
];
const clip: Rule = { method: "clips.get", scope: sameId("id") };
const character = (paramKey: string): Rule => ({ method: "character.get", scope: sameId(paramKey) });

/** Pushed event type → the cached replies it makes stale. */
export const EVENT_RULES: { readonly [T in StreamEvent["type"]]?: readonly Rule[] } = {
  stream_start: ["runs.list", "runs.page", "tournament.list", "tournaments.page"],
  stream_end: ["runs.list", "runs.page", "tournament.list", "tournaments.page", "tournament.results", "ledger.page", "ledger.summary", "review.sessions"],
  segment_end: ledger,
  job: ledger,
};

/** Mutating RPC method → the cached replies to refetch once it succeeds. */
export const MUTATION_RULES: { readonly [M in RpcMethod]?: readonly Rule[] } = {
  "sim.start": ["runs.list", "runs.page", "tournament.list", "tournaments.page"],
  "reconcile.run": ["reconcile.report", "reconcile.list"],
  "plan.choose": ["plan.proposals", "plan.current", "directive.list"],
  "directive.add": ["directive.list"],
  "directive.deactivate": ["directive.list"],
  "lord.chat": ["directive.list"],
  "character.rules.set": ["character.rules", character("character"), "character.list"],
  "character.tone.set": ["character.list", character("id")],
  "clips.scan": ["clips.list", "clips.get"],
  "clips.render": ["clips.list", clip],
  "clips.approve": ["clips.list", clip],
  "clips.reject": ["clips.list", clip],
  "clips.publish": [clip],
  "clips.feedback": [clip],
  // Feedback is optimistic on the page; only the wall's counts refetch, never the whole session.
  "review.feedback.set": ["review.sessions", "review.feedback.list"],
  "tone.lines.add": ["tone.lines.list", "tone.inbox"],
  "tone.lines.set_status": ["tone.lines.list"],
  "tone.directions.set": ["tone.directions.get"],
  "segments.directions.set": ["segments.directions.get"],
  "voice.settings.set": [{ method: "voice.settings.get", scope: sameId("character", "character") }],
  // A key check is per provider, and every character's settings carry the provider rows.
  "voice.verify": ["voice.settings.get"],
  "review.tag": ["review.sessions", { method: "review.session", scope: sameId("session_id", "session_id") }],
};

const methodOf = (r: Rule): RpcMethod => (typeof r === "string" ? r : r.method);

export function eventRules(type: string): readonly Rule[] {
  return EVENT_RULES[type as StreamEvent["type"]] ?? [];
}

export function mutationRules(method: string): readonly Rule[] {
  return MUTATION_RULES[method as RpcMethod] ?? [];
}

/** The methods an event can invalidate (some entries of each, at most). */
export function invalidatedByEvent(type: string): readonly string[] {
  return eventRules(type).map(methodOf);
}

/** The methods a mutation can invalidate (some entries of each, at most). */
export function invalidatedByMutation(method: string): readonly string[] {
  return mutationRules(method).map(methodOf);
}
