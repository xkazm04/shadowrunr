import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { fail, mountPage, nodes, ok, param, settle, type Script } from "../../kit/testing";
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
      case "plan.current":
        return fail("not_found", "no plan chosen yet");
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
    // The daemon persists what plan.choose seals and serves it back as plan.current.
    let stored: ReturnType<typeof genPlan> | undefined;
    const m = mount(["plan"], (method, body) => {
      if (method === "plan.choose") {
        sent.push(body);
        stored = genPlan(["opt-1", "opt-3"]);
        return ok(stored);
      }
      if (method === "plan.current") return stored ? ok(stored) : fail("not_found", "no plan chosen yet");
      return base()(method, body);
    });
    await settle();
    expect(role("reconcile-composer")?.textContent).toContain("No live plan yet");
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes).toHaveLength(6);
    await act(async () => fireEvent.click(boxes[0] as HTMLElement));
    await act(async () => fireEvent.click(boxes[2] as HTMLElement));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Seal this plan" })));
    await settle();
    expect(m.fetch.calls).toContain("plan.choose");
    expect(sent[0]).toContain('"option_ids":["opt-1","opt-3"]');
    expect(role("reconcile-sealed")?.textContent).toContain("Per my last decree");
    // The sealed block came from the refetched plan.current, after plan.choose.
    expect(m.fetch.calls.lastIndexOf("plan.current")).toBeGreaterThan(m.fetch.calls.indexOf("plan.choose"));
    expect(screen.getByRole("button", { name: "This is the live plan" }).hasAttribute("disabled")).toBe(true);
  });

  const checked = (id: string) => (document.querySelector(`[data-option='${id}'] input[type='checkbox']`) as HTMLInputElement | null)?.checked;

  it("after a reload shows the daemon's live plan, seeds the choice from it and will not re-seal it", async () => {
    session.showReport("rec-1792284840627");
    mount(["plan"], base({ "plan.current": genPlan(["opt-1", "opt-3"]) }));
    await settle();
    expect(role("reconcile-sealed")?.textContent).toContain("Per my last decree");
    expect(checked("opt-1")).toBe(true);
    expect(checked("opt-3")).toBe(true);
    expect(checked("opt-4")).toBe(false);
    const btn = screen.getByRole("button", { name: "This is the live plan" });
    expect(btn.hasAttribute("disabled")).toBe(true);
    expect(screen.queryByRole("button", { name: "Seal this plan" })).toBeNull();

    // Untick opt-3: the composer names the difference and offers a replacement, never a silent seal.
    await act(async () => fireEvent.click(document.querySelector("[data-option='opt-3'] input[type='checkbox']") as HTMLElement));
    expect(checked("opt-3")).toBe(false);
    expect(checked("opt-1")).toBe(true);
    expect(role("reconcile-composer")?.textContent).toContain("Differs from the live plan: drops opt-3");
    const replace = screen.getByRole("button", { name: "Replace the live plan" });
    expect(replace.hasAttribute("disabled")).toBe(false);
  });

  it("names the report a live plan was sealed from when it is another one", async () => {
    session.showReport("rec-1792284840627");
    mount(["plan"], base({ "plan.current": { ...genPlan(["opt-1"]), report_id: "rec-OTHER" } }));
    await settle();
    expect(role("reconcile-composer")?.textContent).toContain("The live plan was sealed from report rec-OTHER");
    expect(screen.getAllByRole("checkbox").some((b) => (b as HTMLInputElement).checked)).toBe(false);
    expect(screen.getByRole("button", { name: "Replace the live plan" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Seal this plan" })).toBeNull();
  });

  it("warns when the live plan cannot be read and still allows sealing", async () => {
    session.showReport("rec-1792284840627");
    mount(["plan"], (method, body) => (method === "plan.current" ? { status: 500, body: { error: { code: "engine", message: "disk on fire" } } } : base()(method, body)));
    await settle();
    const caution = role("reconcile-live-unknown");
    expect(caution?.textContent).toContain("Could not read the live plan");
    expect(caution?.textContent).toContain("The engine failed");
    expect(screen.getByRole("button", { name: "Seal this plan" }).hasAttribute("disabled")).toBe(false);
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

  it("shows proposals as Proposed with Keep and Drop over the chat's RPCs, and leaves kept and dropped rows as they were", async () => {
    const kept = { id: "dir-k", kind: "try" as const, text: "kept earlier", created_ms: 1, active: true };
    const off = { id: "dir-o", kind: "prevent" as const, text: "dropped earlier", created_ms: 2, active: false };
    const a = { id: "dir-a", kind: "prevent" as const, text: "repeat the poll line", created_ms: 3, active: false, proposed: true, scope: "SEG-01" as const };
    const b = { id: "dir-b", kind: "try" as const, text: "open with the ledger", created_ms: 4, active: false, proposed: true };
    let list: Record<string, unknown>[] = [kept, off, a, b];
    const bodies: Record<string, string[]> = { "directive.add": [], "directive.deactivate": [] };
    mount(["directives"], (method, body) => {
      if (method === "directive.list") return ok(list);
      if (method === "directive.add") {
        bodies[method]?.push(body);
        const k = { ...a, active: true, proposed: undefined };
        list = list.map((d) => (d.id === "dir-a" ? k : d));
        return ok(k);
      }
      if (method === "directive.deactivate") {
        bodies[method]?.push(body);
        const dropped = { ...b, active: false, proposed: undefined };
        list = list.map((d) => (d.id === "dir-b" ? dropped : d));
        return ok(dropped);
      }
      return base()(method, body);
    });
    await settle();
    // Proposals await the owner, so the page opens on them.
    expect(screen.getByText("2 of 4")).toBeTruthy();
    expect(all("reconcile-directive-proposal").length).toBe(2);
    expect(screen.getAllByText("Proposed").length).toBeGreaterThanOrEqual(2);
    expect(screen.queryByRole("button", { name: "Deactivate" })).toBeNull();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Keep prevent: repeat the poll line" })));
    await settle();
    expect(bodies["directive.add"]?.[0]).toContain('"kind":"prevent"');
    expect(bodies["directive.add"]?.[0]).toContain('"text":"repeat the poll line"');
    expect(bodies["directive.add"]?.[0]).toContain('"scope":"SEG-01"');
    expect(screen.queryByRole("button", { name: "Keep prevent: repeat the poll line" })).toBeNull();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Drop try: open with the ledger" })));
    await settle();
    expect(bodies["directive.deactivate"]?.[0]).toContain('"id":"dir-b"');
    expect(all("reconcile-directive-proposal").length).toBe(0);
    expect(screen.queryByRole("button", { name: /^(Keep|Drop) / })).toBeNull();

    // Every row now shows as a kept or dropped directive always has: Deactivate, or Off.
    await act(async () => fireEvent.click(screen.getByRole("button", { name: /^All/ })));
    await settle();
    const stateOf = (text: string) => screen.getByText(text).closest("tr")?.querySelector("td:last-child")?.textContent;
    expect(stateOf("repeat the poll line")).toBe("Deactivate");
    expect(stateOf("kept earlier")).toBe("Deactivate");
    expect(stateOf("open with the ledger")).toBe("Off");
    expect(stateOf("dropped earlier")).toBe("Off");
  });

  it("virtualises 2,000 directives", async () => {
    mount(["directives"], base({ "directive.list": genDirectives(2_000) }));
    await settle();
    expect(screen.getByText("1,600 of 2,000")).toBeTruthy();
    expect(role("reconcile-directives")?.querySelectorAll("tbody tr").length).toBeLessThan(60);
  });
});

describe("reconcile/chat", () => {
  it("sends to lord.chat and shows the directives the reply proposed as chips", async () => {
    session.showReport("rec-1792284840627");
    const bodies: string[] = [];
    const proposed = { id: "dir-9", kind: "prevent" as const, text: "repeat the poll line", created_ms: 1, active: false, proposed: true };
    mount(["chat"], (method, body) => {
      if (method === "lord.chat") {
        bodies.push(body);
        return ok({ reply: "Forbidden, then. It is written.", directives_added: [proposed] });
      }
      if (method === "directive.list") return ok([proposed]);
      return base()(method, body);
    });
    await settle();
    fireEvent.change(screen.getByLabelText("Message the Lord"), { target: { value: "Never repeat the poll line." } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    await settle();
    expect(bodies[0]).toContain('"message":"Never repeat the poll line."');
    expect(bodies[0]).toContain('"report_id":"rec-1792284840627"');
    expect(screen.getByText("Forbidden, then. It is written.")).toBeTruthy();
    const chip = role("reconcile-chat-directive");
    expect(chip?.getAttribute("data-state")).toBe("proposed");
    expect(screen.getByText("prevent: repeat the poll line")).toBeTruthy();
  });

  it("keeps and drops proposed directives through the directive RPCs", async () => {
    const a = { id: "dir-a", kind: "prevent" as const, text: "repeat the poll line", created_ms: 1, active: false, proposed: true, scope: "SEG-01" as const };
    const b = { id: "dir-b", kind: "try" as const, text: "open with the ledger", created_ms: 2, active: false, proposed: true };
    let list: Record<string, unknown>[] = [a, b];
    const bodies: Record<string, string[]> = { "directive.add": [], "directive.deactivate": [] };
    const m = mount(["chat"], (method, body) => {
      if (method === "lord.chat") return ok({ reply: "Two proposals.", directives_added: [a, b] });
      if (method === "directive.list") return ok(list);
      if (method === "directive.add") {
        bodies[method]?.push(body);
        const kept = { ...a, active: true, proposed: undefined };
        list = [kept, b];
        return ok(kept);
      }
      if (method === "directive.deactivate") {
        bodies[method]?.push(body);
        const dropped = { ...b, active: false, proposed: undefined };
        list = list.map((d) => (d.id === "dir-b" ? dropped : d));
        return ok(dropped);
      }
      return base()(method, body);
    });
    await settle();
    fireEvent.change(screen.getByLabelText("Message the Lord"), { target: { value: "Fix the openings." } });
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Send" })));
    await settle();
    const state = (id: string) => [...all("reconcile-chat-directive")].map((n) => n.getAttribute("data-state"))[id === "a" ? 0 : 1];
    expect(state("a")).toBe("proposed");
    expect(state("b")).toBe("proposed");

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Keep prevent: repeat the poll line" })));
    await settle();
    expect(bodies["directive.add"]?.[0]).toContain('"kind":"prevent"');
    expect(bodies["directive.add"]?.[0]).toContain('"text":"repeat the poll line"');
    expect(bodies["directive.add"]?.[0]).toContain('"scope":"SEG-01"');
    expect(state("a")).toBe("kept");
    expect(screen.queryByRole("button", { name: "Keep prevent: repeat the poll line" })).toBeNull();

    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Drop try: open with the ledger" })));
    await settle();
    expect(bodies["directive.deactivate"]?.[0]).toContain('"id":"dir-b"');
    expect(state("b")).toBe("dropped");
    expect(screen.queryByRole("button", { name: /^(Keep|Drop) / })).toBeNull();
    expect(m.fetch.calls.filter((c) => c === "directive.list").length).toBeGreaterThanOrEqual(3);
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
