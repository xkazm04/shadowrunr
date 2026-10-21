// compareSummaries: the Engine floor's baseline vs candidate, role by role. A delta shows only
// where both sides carry enough calls; a one-sided role is new or gone, never a delta against 0.
import { describe, expect, it } from "vitest";
import { compareSummaries, signedMs, signedPp, STATE_LABEL, type EngineSummary, type ObservedRoute } from "./engineModel";

const route = (role: string, calls: number, o: Partial<ObservedRoute> = {}): ObservedRoute => ({
  role,
  calls,
  errors: 0,
  fallbacks: 0,
  tiers: [],
  efforts: [],
  engines: [],
  models: [],
  durationP50: 1000,
  durationP95: 2000,
  firstTextP50: 500,
  ...o,
});

const summary = (routes: ObservedRoute[], o: Partial<EngineSummary> = {}): EngineSummary => ({
  calls: routes.reduce((a, r) => a + r.calls, 0),
  errors: routes.reduce((a, r) => a + r.errors, 0),
  fallbacks: routes.reduce((a, r) => a + r.fallbacks, 0),
  cost: 1,
  uncosted: 0,
  cacheRead: 0,
  unrouted: 0,
  routes,
  accounts: [],
  runStart: new Map(),
  ...o,
});

describe("compareSummaries", () => {
  it("signs a latency change per role and calls a slower candidate worse", () => {
    const c = compareSummaries(summary([route("core_thread", 200, { durationP50: 1000 })]), summary([route("core_thread", 200, { durationP50: 1400 })]));
    const row = c.rows.find((r) => r.role === "core_thread");
    expect(row?.state).toBe("both");
    expect(row?.p50.delta).toBe(400);
    expect(row?.p50.direction).toBe("worse");
    expect(signedMs(row?.p50.delta)).toBe("+400 ms");
    const faster = compareSummaries(summary([route("core_thread", 200, { durationP50: 1400 })]), summary([route("core_thread", 200, { durationP50: 1000 })]));
    expect(faster.rows[0]?.p50.direction).toBe("better");
  });

  it("names a role on one side only as new or gone, with no delta", () => {
    const c = compareSummaries(summary([route("core_thread", 200), route("judge", 50)]), summary([route("core_thread", 200), route("researcher", 80)]));
    const fresh = c.rows.find((r) => r.role === "researcher");
    const gone = c.rows.find((r) => r.role === "judge");
    expect(fresh?.state).toBe("new");
    expect(fresh?.p50.delta).toBeUndefined();
    expect(fresh?.errorRate.delta).toBeUndefined();
    expect(fresh?.candCalls).toBe(80);
    expect(gone?.state).toBe("gone");
    expect(gone?.p95.delta).toBeUndefined();
    expect(gone?.baseCalls).toBe(50);
  });

  it("refuses a delta on a role under the minimum sample", () => {
    const c = compareSummaries(summary([route("chatter", 12, { durationP50: 900 })]), summary([route("chatter", 200, { durationP50: 3000 })]), { minN: 20 });
    const row = c.rows[0];
    expect(row?.state).toBe("thin");
    expect(STATE_LABEL.thin).toBe("too few calls");
    expect(row?.p50.delta).toBeUndefined();
    expect(row?.p50.direction).toBeUndefined();
    expect(row?.p95.delta).toBeUndefined();
    expect(row?.firstText.delta).toBeUndefined();
    expect(signedMs(row?.p50.delta)).toBe("–");
  });

  it("reads error rate in percentage points and marks a cost delta over uncosted rows as a lower bound", () => {
    const c = compareSummaries(summary([route("core_thread", 100, { errors: 2 })]), summary([route("core_thread", 100, { errors: 10 })]));
    const row = c.rows[0];
    expect(row?.errorRate.delta).toBeCloseTo(8);
    expect(row?.errorRate.direction).toBe("worse");
    expect(signedPp(row?.errorRate.delta)).toBe("+8.0 pp");
    expect(c.total.errorRate.delta).toBeCloseTo(8);
    expect(c.cost.lowerBound).toBe(false);
    expect(c.cost.delta).toBe(0);
    const partly = compareSummaries(summary([route("core_thread", 100)], { cost: 2, uncosted: 3 }), summary([route("core_thread", 100)], { cost: 3 }));
    expect(partly.cost.delta).toBe(1);
    expect(partly.cost.lowerBound).toBe(true);
    const other = compareSummaries(summary([route("core_thread", 100)]), summary([route("core_thread", 100)], { uncosted: 1 }));
    expect(other.cost.lowerBound).toBe(true);
  });
});
