// Compare two sandbox sessions: the pure diff (rules newly failing / fixed / still failing, gate
// checks that flipped, reply-wait and silence against the page's limits, line-mix shifts and
// quality moves) and the `review/<A>/compare/<B>` route with its "Compare with previous" tab.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReviewSession, ReviewSessionRow, SandboxCheck } from "@shadowrunr/contract";
import { mountPage, ok, param, settle } from "../../kit/testing";
import { humanize, pageRoute } from "../../router";
import { checkDiff, dimDiff, figureDiff, mixDiff, previousOf, ruleDiff } from "./compare";
import { reviewLabel } from "./index";
import { Layer2 } from "./Layer2";
import { dimName, ruleName } from "./names";
import { reviewSession } from "./tabs.fixture";

let dispose: (() => void) | null = null;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

const A = "sbx-20261020-120000-30m";
const B = "sbx-20261020-130000-10m";
const OLDEST = "sbx-20261019-090000-5m";
const R1 = "situation:edgy-roast-request";
const R2 = "red-line:no-lecture";
const R3 = "situation:empty-room";

type Verdict = NonNullable<ReviewSession["verdict"]>;

/** A fixture session under another id, start and length, with its judged rules replaced. */
function session(id: string, started: number, minutes: number, opts: { fails?: readonly string[]; passes?: readonly string[]; judged?: boolean; checks?: readonly SandboxCheck[]; quality?: Verdict["quality"] } = {}): ReviewSession {
  const rs = reviewSession(opts.judged === false ? { judged: false } : {});
  const ref = (l: number) => `${id}#${l}`;
  const verdict: Verdict | undefined = rs.verdict && {
    ...rs.verdict,
    run_id: id,
    rules: [
      ...(opts.fails ?? []).map((rule, i) => ({ rule, state: "fail" as const, evidence: [ref(4 + 5 * i)] })),
      ...(opts.passes ?? []).map((rule) => ({ rule, state: "pass" as const })),
    ],
    ...(opts.quality ? { quality: opts.quality } : {}),
  };
  return {
    ...rs,
    row: { ...rs.row, session_id: id, started_ms: started, minutes, ...(opts.checks ? { checks: [...opts.checks] } : {}), ...(verdict ? { rule_fails: opts.fails?.length ?? 0 } : {}) },
    ...(verdict ? { verdict } : {}),
  };
}

const sA = () => session(A, 1_000, 30, { fails: [R1, R2], passes: [R3] });
const sB = () => session(B, 2_000, 10, { fails: [R2, R3], passes: [R1] });

const rowOf = (rs: ReviewSession): ReviewSessionRow => rs.row;

async function mount(sub: string[], sessions: readonly ReviewSession[]) {
  const m = mountPage(
    <Layer2 sub={sub} />,
    (method, sent) => {
      if (method === "review.session") {
        const s = sessions.find((x) => x.row.session_id === param(sent, "session_id"));
        return s ? ok(s) : { status: 404, body: { error: { code: "not_found", message: "no such session" } } };
      }
      if (method === "review.sessions") return ok([...sessions].sort((x, y) => y.row.started_ms - x.row.started_ms).map(rowOf));
      if (method === "review.feedback.list") return ok([]);
      return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    },
    `#/review/${sub.join("/")}`,
  );
  dispose = m.dispose;
  await settle(10);
  return m;
}
const text = () => document.body.textContent ?? "";
const click = (el: Element | null) => act(async () => void (el && fireEvent.click(el)));

describe("rules", () => {
  it("splits A's and B's failures into newly failing, fixed and still failing, refs on their own session", () => {
    const d = ruleDiff(sA(), sB());
    expect(d.newly.map((r) => r.rule)).toEqual([R3]);
    expect(d.fixed.map((r) => r.rule)).toEqual([R1]);
    expect(d.still.map((r) => r.rule)).toEqual([R2]);
    // Newly failing points into B, fixed into A, still failing into both.
    expect(d.newly[0]?.refs.map((r) => r.session)).toEqual([B]);
    expect(d.fixed[0]?.refs.map((r) => r.session)).toEqual([A]);
    expect(new Set(d.still[0]?.refs.map((r) => r.session))).toEqual(new Set([A, B]));
  });

  it("an unjudged B fixes nothing: unmeasured is not a pass", () => {
    const d = ruleDiff(sA(), session(B, 2_000, 10, { judged: false }));
    expect(d.bJudged).toBe(false);
    expect(d.fixed).toEqual([]);
    expect(d.newly).toEqual([]);
    expect(d.still).toEqual([]);
  });

  it("a rule A failed that B did not check is not fixed", () => {
    const d = ruleDiff(sA(), session(B, 2_000, 10, { fails: [R2] }));
    expect(d.fixed).toEqual([]);
    expect(d.unchecked.map((r) => r.rule)).toEqual([R1]);
  });
});

describe("gate checks", () => {
  it("lists only the checks that flipped, in tone, and counts the rest", () => {
    const a: SandboxCheck[] = [
      { id: "say_gap", state: "pass", detail: "" },
      { id: "core_errors", state: "pass", detail: "" },
      { id: "judge", state: "fail", detail: "" },
    ];
    const b: SandboxCheck[] = [
      { id: "say_gap", state: "fail", detail: "" },
      { id: "core_errors", state: "pass", detail: "" },
      { id: "judge", state: "fail", detail: "" },
    ];
    const d = checkDiff(a, b);
    expect(d.changed.map((c) => [c.label, c.tone])).toEqual([["Say gap: passed -> failed", "fail"]]);
    expect(d.unchanged).toBe(2);
    expect(checkDiff(b, a).changed[0]?.tone).toBe("ok");
  });
});

describe("figures", () => {
  it("prints the change and fails the row when B crosses the page's limit", () => {
    const rows = figureDiff({ replyP95: 3.2, silence: 12 }, { replyP95: 5.1, silence: 16.5 });
    const reply = rows.find((r) => r.id === "reply");
    expect(reply?.delta).toBe("+1.9 s");
    expect(reply?.tone).toBe("fail");
    const silence = rows.find((r) => r.id === "silence");
    expect(silence?.delta).toBe("+4.5 s");
    expect(silence?.tone).toBe("fail");
    // Back under the line is a fix; moving under it is neither.
    expect(figureDiff({ replyP95: 5.1, silence: 16 }, { replyP95: 3.2, silence: 14 }).map((r) => r.tone)).toEqual(["ok", "ok"]);
    expect(figureDiff({ replyP95: 1.0, silence: 5 }, { replyP95: 2.0, silence: 6 }).map((r) => r.tone)).toEqual(["neutral", "neutral"]);
    // A side with no replies has no p95 and no change.
    expect(figureDiff({ silence: 5 }, { replyP95: 2.0, silence: 5 })[0]?.delta).toBe("–");
  });
});

describe("line mix", () => {
  it("orders kinds by the size of their share change, in points", () => {
    const d = mixDiff({ mix: { roast: 30, joke: 50, tease: 20 }, lines: 100 }, { mix: { roast: 12, joke: 53, tease: 35 }, lines: 100 });
    expect(d.rows.map((r) => r.label)).toEqual(["Roast -18 pts", "Tease +15 pts", "Joke +3 pts"]);
    expect(d.rows[0]?.a).toBeCloseTo(0.3);
    expect(d.rows[0]?.b).toBeCloseTo(0.12);
  });
});

describe("quality", () => {
  it("lists the dimension that moved most first, with both rates", () => {
    const q = (dim: string, yes: number, n: number, run: string) =>
      Array.from({ length: n }, (_, i) => ({ dimension: dim, value: i < yes, quote: "", turn_ref: `${run}#${4 + i}` }));
    const a = session(A, 1_000, 30, { fails: [R1], quality: [...q("humor_landed", 4, 5, A), ...q("in_character", 5, 5, A)] });
    const b = session(B, 2_000, 10, { fails: [R1], quality: [...q("humor_landed", 1, 5, B), ...q("in_character", 4, 5, B)] });
    const rows = dimDiff(a, b);
    expect(rows.map((r) => r.dim)).toEqual(["humor_landed", "in_character"]);
    expect(rows[0]?.name).toBe(dimName("humor_landed"));
    expect([rows[0]?.a?.yes, rows[0]?.a?.total, rows[0]?.b?.yes, rows[0]?.b?.total]).toEqual([4, 5, 1, 5]);
  });
});

describe("the compare route", () => {
  it("renders both sessions; an unknown second id is no page", async () => {
    await mount([A, "compare", B], [sA(), sB()]);
    const sides = [...document.querySelectorAll("[data-role='review-compare-side']")];
    expect(sides).toHaveLength(2);
    expect(sides[0]?.textContent).toContain("30-minute session");
    expect(sides[0]?.textContent).toContain(A);
    expect(sides[1]?.textContent).toContain("10-minute session");
    expect(sides[1]?.textContent).toContain(B);
    expect(text()).not.toContain("There is no page at");
    cleanup();
    dispose?.();
    await mount([A, "compare", "nope"], [sA(), sB()]);
    expect(text()).toContain("There is no page at");
    expect(document.querySelector("[data-role='review-compare-side']")).toBeNull();
  });

  it("lists the rule diff by name, each ref opening its own session's timeline line", async () => {
    const { router } = await mount([A, "compare", B], [sA(), sB()]);
    const group = (k: string) => document.querySelector(`[data-role='review-compare-${k}']`);
    expect(group("newly")?.textContent).toContain(`Newly failing${ruleName(R3)}`);
    expect(group("fixed")?.textContent).toContain(`Fixed${ruleName(R1)}`);
    expect(group("still")?.textContent).toContain(`Still failing${ruleName(R2)}`);
    await click(group("newly")?.querySelector("button[data-line]") ?? null);
    expect(router.getRoute()).toEqual(pageRoute("review", B, "timeline", "9"));
    await click(group("fixed")?.querySelector("button[data-line]") ?? null);
    expect(router.getRoute()).toEqual(pageRoute("review", A, "timeline", "4"));
  });

  it("says B was not judged, lists no fix and never claims no rule failed", async () => {
    await mount([A, "compare", B], [sA(), session(B, 2_000, 10, { judged: false })]);
    expect(text()).toContain("B was not judged");
    expect(document.querySelector("[data-role='review-compare-fixed']")).toBeNull();
    expect(text()).not.toContain("No rule failed");
  });

  it("prints flipped checks, the figures and the mix", async () => {
    const checks = (s: SandboxCheck["state"]): SandboxCheck[] => [
      { id: "say_gap", state: s, detail: "" },
      { id: "judge", state: "fail", detail: "" },
    ];
    await mount([A, "compare", B], [session(A, 1_000, 30, { fails: [R1], checks: checks("pass") }), session(B, 2_000, 10, { fails: [R1], checks: checks("fail") })]);
    expect(text()).toContain("Say gap: passed -> failed");
    expect(text()).toContain("1 check unchanged");
    expect(text()).toContain("Reply wait p95");
    expect(text()).toContain("Longest silence");
  });

  it("names the second session in the breadcrumbs", () => {
    // The shell humanizes a segment the page does not name: `compare` reads Compare.
    expect(reviewLabel("compare", 2, ["review", A, "compare", B]) ?? humanize("compare")).toBe("Compare");
    expect(reviewLabel("judge", 2, ["review", A, "judge"]) ?? humanize("judge")).toBe("Judge");
    expect(reviewLabel("types", 2, ["review", A, "types"])).toBe("Review by type");
    expect(reviewLabel(B, 3, ["review", A, "compare", B])).toBe(B);
  });
});

describe("compare with previous", () => {
  it("the tab bar opens the compare against the session started just before; the oldest has none", async () => {
    const all = [session(OLDEST, 500, 5, { fails: [R1] }), sA(), sB()];
    const { router } = await mount([B, "timeline"], all);
    await click(screen.getByRole("tab", { name: "Compare with previous" }));
    expect(router.getRoute()).toEqual(pageRoute("review", A, "compare", B));
    cleanup();
    dispose?.();
    await mount([OLDEST, "timeline"], all);
    expect(screen.queryByRole("tab", { name: "Compare with previous" })).toBeNull();
  });

  it("previousOf picks by started_ms, not list order", () => {
    const rows = [sA().row, session(OLDEST, 500, 5).row, sB().row];
    expect(previousOf(rows, B)?.session_id).toBe(A);
    expect(previousOf(rows, A)?.session_id).toBe(OLDEST);
    expect(previousOf(rows, OLDEST)).toBeUndefined();
    expect(previousOf(rows, "nope")).toBeUndefined();
  });
});
