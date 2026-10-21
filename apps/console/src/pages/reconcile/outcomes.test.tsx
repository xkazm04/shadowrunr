// Each directive's judged outcome (`ReconcileReport.directives`) beside it on the Directives tab,
// and worst first on the Report tab. Mounted like layer2.test.tsx, over the same scripted daemon.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fail, mountPage, ok, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { session } from "./session";
import { genDirectives, genOutcome, genProposals, genReport } from "./testgen";

let dispose: (() => void) | undefined;
beforeEach(() => session.reset());
afterEach(() => {
  cleanup();
  dispose?.();
});

const role = (r: string) => document.querySelector(`[data-role='${r}']`);

function mount(sub: string[], script: Script) {
  const m = mountPage(<Layer2 sub={sub} />, script, `#/reconcile/${sub.join("/")}`);
  dispose = () => m.router.dispose();
  return m;
}

const base =
  (over: Partial<Record<string, unknown>> = {}): Script =>
  (method) => {
    if (method in over) return ok(over[method]);
    switch (method) {
      case "plan.current":
        return fail("not_found", "no plan chosen yet");
      case "reconcile.report":
        return ok(genReport());
      case "plan.proposals":
        return ok(genProposals(6));
      case "directive.list":
        return ok(genDirectives(3));
      default:
        return ok([]);
    }
  };

describe("reconcile/report: the Lord's directives", () => {
  it("lists the Lord's directives worst first, each by its directive.list text or as retired", async () => {
    session.showReport("rec-1792284840627");
    const report = {
      ...genReport(),
      directives: [
        genOutcome("dir-0", "held", 4, 4),
        genOutcome("dir-9", "held", 3, 3),
        genOutcome("dir-1", "broken", 2, 4, { refs: ["run-1#12"] }),
        genOutcome("dir-3", "ignored", 0, 3, { kind: "try" }),
      ],
    };
    mount(["report"], base({ "reconcile.report": report, "directive.list": genDirectives(4) }));
    await settle();
    const section = role("reconcile-report-directives");
    expect(section?.textContent).toContain("The Lord’s directives");
    const rows = [...(section?.querySelectorAll("[data-directive]") ?? [])];
    expect(rows.map((r) => r.getAttribute("data-verdict"))).toEqual(["broken", "ignored", "held", "held"]);
    expect(rows.map((r) => r.querySelector("[data-role='reconcile-directive-name']")?.textContent)).toEqual([
      "Directive number 1",
      "Directive number 3",
      "Directive number 0",
      "retired directive dir-9",
    ]);
    expect(rows[0]?.textContent).toContain("held 2 of 4");
    expect(screen.getByLabelText("held: 50% (15%–85%), n=4")).toBeTruthy();
    expect([...(rows[0]?.querySelectorAll("a") ?? [])].map((a) => a.getAttribute("href"))).toContain("#/brain/transcripts/run-1/12");
  });
});

describe("reconcile/directives: outcomes", () => {
  const outcomeOf = (text: string) => screen.getByText(text).closest("tr")?.querySelector("[data-role='reconcile-directive-outcome']");

  it("puts each directive's judged outcome beside it, from the report on show", async () => {
    session.showReport("rec-1792284840627");
    const report = {
      ...genReport(),
      directives: [genOutcome("dir-1", "broken", 2, 4, { refs: ["run-1#12"] }), genOutcome("dir-3", "attempted", 1, 1, { kind: "try", lo: 0.05, hi: 1 })],
    };
    mount(["directives"], base({ "reconcile.report": report, "directive.list": genDirectives(4) }));
    await settle();
    expect(role("reconcile-outcomes-source")?.textContent).toContain("rec-1792284840627");
    const broken = outcomeOf("Directive number 1");
    expect(broken?.getAttribute("data-verdict")).toBe("broken");
    expect(broken?.textContent).toContain("Broken");
    expect(broken?.textContent).toContain("held 2 of 4");
    expect(broken?.textContent).toContain("95% 15%–85%");
    expect(broken?.querySelector("a")?.getAttribute("href")).toBe("#/brain/transcripts/run-1/12");
    expect(broken?.querySelector("[data-role='reconcile-small-sample']")).toBeTruthy();

    // A directive no run judged is never shown as held.
    const unjudged = outcomeOf("Directive number 2");
    expect(unjudged?.textContent).toContain("Not judged in rec-1792284840627");
    expect(unjudged?.textContent).not.toMatch(/held/i);

    // 1 of 1 attempted carries the small-sample mark, so it does not read as settled.
    const one = outcomeOf("Directive number 3");
    expect(one?.textContent).toContain("attempted 1 of 1");
    expect(one?.querySelector("[data-role='reconcile-small-sample']")).toBeTruthy();
  });

  it("filters to the directives the character broke, each still with its Deactivate", async () => {
    session.showReport("rec-1792284840627");
    const report = { ...genReport(), directives: [genOutcome("dir-1", "broken", 2, 4), genOutcome("dir-2", "held", 4, 4), genOutcome("dir-0", "broken", 1, 4)] };
    mount(["directives"], base({ "reconcile.report": report, "directive.list": genDirectives(4) }));
    await settle();
    // dir-0 is off (i % 5 === 0): only active directives count as broken here.
    const pill = screen.getByRole("button", { name: /^Broken/ });
    expect(pill.textContent).toBe("Broken1");
    await act(async () => fireEvent.click(pill));
    await settle();
    const rows = role("reconcile-directives")?.querySelectorAll("tbody tr") ?? [];
    const texts = [...rows].map((r) => r.textContent ?? "").filter((t) => t.includes("Directive number"));
    expect(texts).toHaveLength(1);
    expect(texts[0]).toContain("Directive number 1");
    expect(screen.getByRole("button", { name: "Deactivate" })).toBeTruthy();
  });

  it("shows a dash for the outcome when no report is stored, and no error", async () => {
    mount(["directives"], base());
    await settle();
    expect(outcomeOf("Directive number 1")?.textContent).toBe("–");
    expect(role("reconcile-outcomes-source")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
