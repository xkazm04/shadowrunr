import { cleanup, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mountPage, nodes, ok, settle, type Script } from "./harness";
import { Glance, Motif } from "./Face";
import { Layer1 } from "./Layer1";
import { session } from "./session";
import { genProposals, genReport } from "./testgen";

let dispose: (() => void) | undefined;

beforeEach(() => session.reset());
afterEach(() => {
  cleanup();
  dispose?.();
});

const daemon =
  (report = genReport(), proposals = genProposals(6)): Script =>
  (method) =>
    method === "reconcile.report" ? ok(report) : method === "plan.proposals" ? ok(proposals) : ok([]);

const role = (r: string) => document.querySelector(`[data-role='${r}']`);

describe("layer 1: the triptych", () => {
  it("renders nothing while hidden", () => {
    const m = mountPage(<Layer1 active={false} />, daemon());
    dispose = () => m.router.dispose();
    expect(role("reconcile-layer1")).toBeNull();
  });

  it("shows the designed empty state, with Run reconcile, when there is no report", async () => {
    const m = mountPage(<Layer1 active />, daemon());
    dispose = () => m.router.dispose();
    await settle();
    expect(role("reconcile-triptych")?.textContent).toContain("–");
    expect(screen.getByText("No report yet.")).toBeTruthy();
    expect(role("reconcile-run")?.getAttribute("href")).toBe("#/reconcile/report");
    expect(m.fetch.calls).not.toContain("reconcile.report");
  });

  it("counts held, failed and proposed from the report and links each into layer 2", async () => {
    session.showReport("rec-1792284840627");
    const m = mountPage(<Layer1 active />, daemon());
    dispose = () => m.router.dispose();
    await settle();
    const n = (k: string) => role(`reconcile-${k}`)?.querySelector("[data-n]");
    expect(n("held")?.getAttribute("data-n")).toBe("14");
    expect(n("failed")?.getAttribute("data-n")).toBe("13");
    expect(n("proposed")?.getAttribute("data-n")).toBe("6");
    expect(role("reconcile-held")?.getAttribute("href")).toBe("#/reconcile/report");
    expect(role("reconcile-failed")?.getAttribute("href")).toBe("#/reconcile/report/failed");
    expect(role("reconcile-proposed")?.getAttribute("href")).toBe("#/reconcile/plan");
    expect(role("reconcile-held-bars")?.children).toHaveLength(14);
    expect(role("reconcile-caution")?.textContent).toBe("Handle with care: all 4 judge calls errored and most rates rest on small samples.");
    expect(role("reconcile-cta")?.getAttribute("href")).toBe("#/reconcile/plan");
    expect(role("reconcile-sun")).toBeTruthy();
  });

  it("prints the final numerals at once under reduced motion", async () => {
    const saved = window.matchMedia;
    window.matchMedia = ((q: string) => ({ matches: q.includes("reduce"), media: q })) as typeof window.matchMedia;
    try {
      session.showReport("rec-1792284840627");
      const m = mountPage(<Layer1 active />, daemon());
      dispose = () => m.router.dispose();
      await settle();
      expect(role("reconcile-held")?.querySelector("[data-n]")?.textContent).toBe("14");
      expect(role("reconcile-failed")?.querySelector("[data-n]")?.textContent).toBe("13");
    } finally {
      window.matchMedia = saved;
    }
  });

  it("shows the offline state when the daemon does not answer", async () => {
    session.showReport("rec-1");
    const m = mountPage(<Layer1 active />, () => ({ network: true }));
    dispose = () => m.router.dispose();
    await settle();
    expect(role("reconcile-caution")?.textContent).toContain("The daemon is not reachable");
    expect(screen.getByText("Retry")).toBeTruthy();
  });

  it("holds its DOM at 10x: 2,000 held, 2,000 failed, 4,170 incidents", async () => {
    session.showReport("rec-big");
    const m = mountPage(<Layer1 active />, daemon(genReport({ id: "rec-big", held: 2_000, failed: 2_000, incidents: 4_170 }), genProposals(60)));
    dispose = () => m.router.dispose();
    await settle();
    expect(role("reconcile-held")?.querySelector("[data-n]")?.getAttribute("data-n")).toBe("2000");
    expect(role("reconcile-held-bars")?.children).toHaveLength(60);
    const count = nodes(role("reconcile-layer1") ?? document.body);
    expect(count).toBeLessThan(260);
    console.info(`dom-count reconcile.l1@10x: ${count}`);
  });
});

describe("the poster face", () => {
  it("draws the motif and glances the counts", async () => {
    session.showReport("rec-1792284840627");
    const m = mountPage(
      <>
        <Motif />
        <span data-role="glance">
          <Glance />
        </span>
      </>,
      daemon(),
    );
    dispose = () => m.router.dispose();
    await settle();
    expect(role("reconcile-motif")?.tagName.toLowerCase()).toBe("svg");
    expect(role("glance")?.textContent).toBe("14 held13 failed6 proposed");
  });
});
