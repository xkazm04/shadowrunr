import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mountPage, nodes, ok, param, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { session } from "./session";
import { genDirectives, genPlan, genProposals, genReport, wireReport } from "./testgen";

let dispose: (() => void) | undefined;
beforeEach(() => session.reset());
afterEach(() => {
  cleanup();
  dispose?.();
});

const role = (r: string) => document.querySelector(`[data-role='${r}']`);
const all = (r: string) => document.querySelectorAll(`[data-role='${r}']`);

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
      case "reconcile.report":
        return ok(genReport());
      case "plan.proposals":
        return ok(genProposals(6));
      case "directive.list":
        return ok(genDirectives(3));
      case "tournament.list":
        return ok([{ tournament_id: "pilot-d1ef98d3a6", instrument_hash: "abc", runs: 4, completed: 4 }]);
      default:
        return ok([]);
    }
  };

describe("reconcile/report", () => {
  it("shows the designed empty state and runs reconcile into the report", async () => {
    const m = mount(["report"], (method, sent) => (method === "reconcile.run" && sent.includes('"tournament_id":"pilot-d1ef98d3a6"') ? ok({ report_id: "rec-1792284840627" }) : base()(method, sent)));
    await settle();
    expect(role("reconcile-empty")).toBeTruthy();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Run reconcile" })));
    await settle();
    expect(m.fetch.calls).toContain("reconcile.run");
    expect(screen.getByText("My Lord, Malakor's core held up in all four runs.")).toBeTruthy();
  });

  it("links evidence into Brain transcripts by route, and filters to what failed", async () => {
    session.showReport("rec-1792284840627");
    mount(["report"], base());
    await settle();
    const refs = [...all("reconcile-ref")].map((a) => a.getAttribute("href"));
    expect(refs).toContain("#/brain/transcripts/theatrical.haiku.SEG-01.a3.s1/1");
    expect(refs).toContain("#/brain/transcripts/pilot-1%2Ftheatrical.sonnet.SEG-01.a30.s1/5");
    expect(screen.getAllByText(/What held/).length).toBeGreaterThan(0);
    cleanup();
    dispose?.();
    mount(["report", "failed"], base());
    await settle();
    expect(screen.queryByText(/What held/)).toBeNull();
    expect(screen.getAllByText(/What failed/).length).toBe(2);
    expect(screen.getByRole("link", { name: "Failed 13" }).getAttribute("aria-current")).toBe("true");
  });

  it("reads a cut report: counts for every incident, the rest paged in by rule, and what the prompt left out", async () => {
    session.showReport("rec-cut");
    const full = genReport({ id: "rec-cut", incidents: 1_200 });
    const wire = wireReport(full);
    const m = mount(["report"], (method, sent) => {
      if (method === "reconcile.report") return ok(wire);
      if (method === "reconcile.incidents") {
        const rule = param(sent, "rule");
        const items = full.red_lines.filter((i) => rule === undefined || i.rule === rule);
        const start = Number(param(sent, "cursor") ?? "0");
        const end = start + 500;
        return ok({ items: items.slice(start, end), total: items.length, ...(end < items.length ? { next_cursor: String(end) } : {}) });
      }
      return base()(method, sent);
    });
    await settle(8);
    expect(screen.getByText("1,200 logged")).toBeTruthy();
    expect(role("reconcile-incidents")?.textContent).toContain("400");
    expect(role("reconcile-incidents-paged")?.textContent).toContain("Showing 500 of 1,200");
    expect(m.fetch.calls.filter((c) => c === "reconcile.incidents")).toHaveLength(1);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: /^rate cap/i })));
    await settle(8);
    expect(role("reconcile-incidents-paged")?.textContent).toContain("Showing 400 of 400 for rate cap");
    expect(m.fetch.calls.filter((c) => c === "reconcile.incidents").length).toBe(2);
    expect(role("reconcile-synthesis-state")?.textContent).toContain("The prompt was cut to fit its budget");
    expect(role("reconcile-synthesis")?.querySelectorAll("tr[data-omitted]").length).toBe(4);
  });

  it("shows ranks and tone quality with n and bound", async () => {
    session.showReport("rec-1792284840627");
    mount(["report"], base());
    await settle();
    expect(screen.getByText("Which ranks got answered")).toBeTruthy();
    expect(screen.getByLabelText("Picked: 49% (45%–53%), n=597")).toBeTruthy();
    expect(screen.getByLabelText("humor landed: 61% (46%–74%), n=41")).toBeTruthy();
  });

  it("holds its DOM at 10x: 2,000 held, 2,000 failed (paged), 4,170 incidents (virtualised)", async () => {
    session.showReport("rec-big");
    mount(["report"], base({ "reconcile.report": genReport({ id: "rec-big", held: 2_000, failed: 2_000, incidents: 4_170 }) }));
    await settle();
    expect(screen.getByText("4,170 of 4,170")).toBeTruthy();
    expect(role("reconcile-report")?.querySelectorAll("tbody tr").length).toBeLessThan(80);
    const count = nodes(role("reconcile-layer2") ?? document.body);
    console.info(`dom-count reconcile.l2/report@10x: ${count}`);
    expect(count).toBeLessThan(1_200);
  });

  it("shows the offline state when the daemon is down", async () => {
    session.showReport("rec-1");
    mount(["report"], () => ({ network: true }));
    await settle();
    expect(screen.getByText("The daemon is not reachable")).toBeTruthy();
  });

  it("names an unknown nested page instead of guessing", async () => {
    mount(["report", "nope"], base());
    await settle();
    expect(screen.getByText("There is no page at report / nope")).toBeTruthy();
  });
});

describe("reconcile/plan", () => {
  it("chooses options, seals them with plan.choose and shows the dispatch", async () => {
    session.showReport("rec-1792284840627");
    const sent: string[] = [];
    const m = mount(["plan"], (method, body) => {
      if (method === "plan.choose") {
        sent.push(body);
        return ok(genPlan(["opt-1", "opt-3"]));
      }
      return base()(method, body);
    });
    await settle();
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes).toHaveLength(6);
    await act(async () => fireEvent.click(boxes[0] as HTMLElement));
    await act(async () => fireEvent.click(boxes[2] as HTMLElement));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Seal this plan" })));
    await settle();
    expect(m.fetch.calls).toContain("plan.choose");
    expect(sent[0]).toContain('"option_ids":["opt-1","opt-3"]');
    expect(role("reconcile-sealed")?.textContent).toContain("Per my last decree");
  });

  it("flags a diff that touches the constitution and refuses the option", async () => {
    session.showReport("rec-1792284840627");
    mount(["plan"], base());
    await settle();
    expect(all("reconcile-constitution-flag")).toHaveLength(1);
    const blocked = document.querySelector("[data-option='opt-2'] input[type='checkbox']");
    expect(blocked?.hasAttribute("disabled")).toBe(true);
    expect(screen.getAllByText("+When chat goes quiet he deals a fresh premise.").length).toBe(1);
  });
});

describe("reconcile/directives", () => {
  it("lists, adds and deactivates directives over RPC", async () => {
    let list = genDirectives(3);
    const bodies: string[] = [];
    const m = mount(["directives"], (method, body) => {
      if (method === "directive.list") return ok(list);
      if (method === "directive.add") {
        bodies.push(body);
        const d = { id: "dir-new", kind: "prevent" as const, text: "no repeated status lines", created_ms: 1792284999999, active: true, scope: "SEG-01" as const };
        list = [...list, d];
        return ok(d);
      }
      if (method === "directive.deactivate") {
        bodies.push(body);
        list = list.map((d) => (d.id === "dir-1" ? { ...d, active: false } : d));
        return ok(list.find((d) => d.id === "dir-1"));
      }
      return base()(method, body);
    });
    await settle();
    expect(screen.getByText("Directive number 1")).toBeTruthy();
    fireEvent.change(screen.getByPlaceholderText(/never repeat the same status line/), { target: { value: "no repeated status lines" } });
    fireEvent.change(screen.getByDisplayValue("All segments"), { target: { value: "SEG-01" } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Add directive" })));
    await settle();
    expect(bodies[0]).toContain('"text":"no repeated status lines"');
    expect(bodies[0]).toContain('"scope":"SEG-01"');
    expect(screen.getByText("no repeated status lines")).toBeTruthy();
    const row = screen.getByText("Directive number 1").closest("tr");
    const btn = row?.querySelector("button");
    await act(async () => fireEvent.click(btn as HTMLElement));
    await settle();
    expect(bodies[1]).toContain('"id":"dir-1"');
    expect(m.fetch.calls.filter((c) => c === "directive.list").length).toBeGreaterThanOrEqual(3);
  });

  it("virtualises 2,000 directives", async () => {
    mount(["directives"], base({ "directive.list": genDirectives(2_000) }));
    await settle();
    expect(screen.getByText("1,600 of 2,000")).toBeTruthy();
    expect(role("reconcile-directives")?.querySelectorAll("tbody tr").length).toBeLessThan(60);
  });
});

describe("reconcile/chat", () => {
  it("sends to lord.chat and shows the directives the reply created as chips", async () => {
    session.showReport("rec-1792284840627");
    const bodies: string[] = [];
    mount(["chat"], (method, body) => {
      if (method === "lord.chat") {
        bodies.push(body);
        return ok({ reply: "Forbidden, then. It is written.", directives_added: [{ id: "dir-9", kind: "prevent", text: "repeat the poll line", created_ms: 1, active: true }] });
      }
      return base()(method, body);
    });
    await settle();
    fireEvent.change(screen.getByLabelText("Message the Lord"), { target: { value: "Never repeat the poll line." } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    await settle();
    expect(bodies[0]).toContain('"message":"Never repeat the poll line."');
    expect(bodies[0]).toContain('"report_id":"rec-1792284840627"');
    expect(screen.getByText("Forbidden, then. It is written.")).toBeTruthy();
    expect(role("reconcile-chat-directives")?.textContent).toBe("prevent: repeat the poll line");
  });

  it("shows an undelivered message when the daemon fails", async () => {
    mount(["chat"], (method, body) => (method === "lord.chat" ? { status: 500, body: { error: { code: "engine", message: "cli exited 1" } } } : base()(method, body)));
    await settle();
    fireEvent.change(screen.getByLabelText("Message the Lord"), { target: { value: "Hello" } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    await settle();
    expect(screen.getByText("Not delivered")).toBeTruthy();
    expect(screen.getByText(/cli exited 1/)).toBeTruthy();
  });
});
