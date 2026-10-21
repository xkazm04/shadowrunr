// The session wall in jsdom: every row field, the order control, the two panels, the empty,
// offline and degraded states, and the wall+l1 DOM budget at 100 sessions.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import DOM_BUDGETS from "../../../budgets/dom.json";
import { fail, mountPage, nodes, offline, ok, role, roles, settle, type Script } from "../../kit/testing";
import { feedbackRow, sessionRows } from "./fixture";
import { Layer1, sortRows } from "./Layer1";

const L1_BUDGET: number = DOM_BUDGETS.routes["wall+l1"];
const WALL: number = DOM_BUDGETS.routes.wall;

afterEach(cleanup);

const daemon =
  (rows = sessionRows(6), feedback = [feedbackRow(4, ["landed", "great_roast"], "sbx-002"), feedbackRow(9, ["landed"], "sbx-003"), feedbackRow(2, ["too_mean"], "sbx-003")]): Script =>
  (method) =>
    method === "review.sessions" ? ok(rows) : method === "review.feedback.list" ? ok(feedback) : ok([]);

async function mount(script: Script = daemon()) {
  const m = mountPage(<Layer1 active />, script, "#/review");
  await settle(8);
  return m;
}

const rowIds = () => roles("review-wall-row").map((r) => /sbx-\d{3}/.exec(r.textContent ?? "")?.[0]);

describe("review wall", () => {
  it("renders nothing while hidden", () => {
    mountPage(<Layer1 active={false} />, daemon());
    expect(role("review-layer1")).toBeNull();
  });

  it("shows one row per session with every field", async () => {
    await mount();
    const rows = roles("review-wall-row");
    expect(rows).toHaveLength(6);
    const first = rows[0] as HTMLElement;
    expect(first.getAttribute("href")).toBe("#/review/sbx-000");
    expect(first.textContent).toContain("5-minute session");
    expect(first.textContent).toContain("passed");
    expect(first.querySelectorAll("li[data-state]")).toHaveLength(6);
    expect(first.querySelector("li[data-state='fail']")?.getAttribute("title")).toBe("Say gap: failed. Longest silence 150.0 s (limit 15 s)");
    expect(first.querySelector("li[data-state='unmeasured']")?.getAttribute("title")).toContain("Chaos: not measured");
    // Top six shares in the key; every kind in the bar, lines with no tag as untagged.
    expect(first.querySelectorAll("ul[aria-hidden] li")).toHaveLength(6);
    expect(first.querySelector("[role='img']")?.children).toHaveLength(8);
    expect(first.querySelector("[role='img'] span[title^='Untagged']")?.getAttribute("title")).toBe("Untagged: 20 lines, 20%");
    expect(first.textContent).toContain("Rule fails–");
    expect(first.textContent).toContain("You marked0 of 100");
    const second = rows[1] as HTMLElement;
    expect(second.textContent).toContain("10-minute session · engine outage");
    expect(second.textContent).toContain("Not tagged yet");
    expect(second.textContent).toContain("Rule fails1");
    // A silence over 15 s is in the fail colour.
    const silences = roles("review-wall-silence");
    expect(silences.map((s) => s.textContent)).toEqual(["8.0 s", "9.0 s", "10.0 s", "11.0 s", "12.0 s", "13.0 s"]);
  });

  it("orders by time, by rule fails and by roast share", async () => {
    await mount();
    expect(rowIds()).toEqual(["sbx-000", "sbx-001", "sbx-002", "sbx-003", "sbx-004", "sbx-005"]);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Most rule fails" })));
    const fails = rowIds().map(order);
    expect([...fails].sort((a, b) => b - a)).toEqual(fails);
    expect(fails[fails.length - 1]).toBe(-1);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Most roasting" })));
    const rows = sessionRows(6);
    const share = (id: string | undefined) => {
      const r = rows.find((x) => x.session_id === id);
      return r ? (r.mix["roast"] ?? 0) / r.lines : 0;
    };
    const shares = rowIds().map(share);
    expect([...shares].sort((a, b) => b - a)).toEqual(shares);
    expect(screen.getByRole("button", { name: "Most roasting" }).getAttribute("aria-pressed")).toBe("true");
  });

  it("totals the line mix across sessions and your marks across them", async () => {
    await mount();
    const all = role("review-wall-all") as HTMLElement;
    const lines = sessionRows(6).reduce((n, r) => n + r.lines, 0);
    // The poster's numerals: the session count with its word, then the lines under it.
    expect(all.querySelector("b")?.textContent).toBe("6");
    expect(all.textContent).toContain(`Sessions${lines.toLocaleString("en-US")} lines from Malakor`);
    // The untagged session's lines show as untagged.
    expect(all.textContent).toContain("Untagged");
    const fb = role("review-wall-feedback") as HTMLElement;
    expect(fb.querySelector("b")?.textContent).toBe("3");
    expect(fb.textContent).toContain("Lines marked");
    expect(fb.textContent).toContain("Landed 2");
    expect(fb.textContent).toContain("Great roast 1");
    expect(fb.textContent).toContain("Too mean 1");
    expect(role("review-cta")?.getAttribute("href")).toBe("#/review/sbx-005");
  });

  it("says nothing is marked yet without feedback", async () => {
    await mount(daemon(sessionRows(2), []));
    expect(role("review-wall-feedback")?.textContent).toContain("Nothing marked yet");
  });

  it("shows the empty state with the command to run", async () => {
    await mount(daemon([], []));
    expect(screen.getByText("No sandbox sessions yet")).toBeTruthy();
    expect(document.querySelector("[data-state='empty']")?.textContent).toContain("shadowrunr stream sandbox --minutes 5");
  });

  it("shows the offline state", async () => {
    await mount(offline);
    expect(screen.getByText("The daemon is not reachable")).toBeTruthy();
  });

  it("keeps the last good rows under a degraded banner when a reload fails", async () => {
    let n = 0;
    const base = daemon();
    const m = await mount((method, sent) => (method === "review.sessions" && ++n > 1 ? fail("io", "disk gone") : base(method, sent)));
    await act(async () => m.layer.cache.refetch("review.sessions", {}));
    await settle(8);
    expect(document.querySelector("[data-state='degraded']")?.textContent).toMatch(/Showing the reply from \d\d:\d\d:\d\d · /);
    expect(roles("review-wall-row")).toHaveLength(6);
  });

  it("holds the wall+l1 DOM budget with 100 sessions", async () => {
    await mount(daemon(sessionRows(100)));
    const shown = roles("review-wall-row").length;
    expect(shown).toBeGreaterThan(0);
    expect(shown).toBeLessThan(20);
    const count = nodes();
    console.info(`dom-count review.l1 at 100 sessions: ${count} (+ wall ${WALL})`);
    expect(count + WALL).toBeLessThan(L1_BUDGET);
  });
});

/** Rule fails as the order sorts them (absent first-last). */
function order(id: string | undefined): number {
  return sessionRows(6).find((r) => r.session_id === id)?.rule_fails ?? -1;
}

describe("sortRows", () => {
  it("breaks ties by time", () => {
    const rows = sessionRows(10);
    const byFails = sortRows(rows, "fails");
    for (let i = 1; i < byFails.length; i++) {
      const a = byFails[i - 1];
      const b = byFails[i];
      if (!a || !b) continue;
      expect((a.rule_fails ?? -1) >= (b.rule_fails ?? -1)).toBe(true);
      if ((a.rule_fails ?? -1) === (b.rule_fails ?? -1)) expect(a.started_ms).toBeLessThan(b.started_ms);
    }
  });
});
