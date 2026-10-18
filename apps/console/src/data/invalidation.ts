// Event-driven cache invalidation: which cached RPC methods an event or a mutation makes stale.
// Polling is only the slow backstop in `QueryCache`.

/** Pushed event type → RPC methods whose cached replies it makes stale. */
export const EVENT_INVALIDATES: Readonly<Record<string, readonly string[]>> = {
  stream_start: ["runs.list", "runs.page", "tournament.list", "tournaments.page"],
  stream_end: ["runs.list", "runs.page", "tournament.list", "tournaments.page", "tournament.results", "ledger.page"],
  segment_end: ["ledger.page"],
  job: ["ledger.page"],
};

/** Mutating RPC method → methods to refetch once it succeeds. */
export const MUTATION_INVALIDATES: Readonly<Record<string, readonly string[]>> = {
  "sim.start": ["runs.list", "runs.page", "tournament.list", "tournaments.page"],
  "reconcile.run": ["reconcile.report", "reconcile.list"],
  "plan.choose": ["plan.proposals", "plan.current", "directive.list"],
  "directive.add": ["directive.list"],
  "directive.deactivate": ["directive.list"],
  "lord.chat": ["directive.list"],
  "character.rules.set": ["character.rules", "character.get", "character.list"],
  "character.tone.set": ["character.list", "character.get"],
};

export function invalidatedByEvent(type: string): readonly string[] {
  return EVENT_INVALIDATES[type] ?? [];
}

export function invalidatedByMutation(method: string): readonly string[] {
  return MUTATION_INVALIDATES[method] ?? [];
}
