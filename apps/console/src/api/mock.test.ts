import { describe, expect, it } from "vitest";
import type { Directive, ReconcileReport, StreamEvent } from "../contract";
import { parseRef } from "../lib/format";
import { annotationsByLine } from "../lib/transcript";
import deskFixture from "../fixtures/desk.json";
import simFixture from "../fixtures/sim.json";
import transcriptFixture from "../fixtures/transcripts.json";
import { mockTransport, transcriptFor } from "./mock";
import { RpcClient, RpcError } from "./rpc";

const client = () => new RpcClient(mockTransport({ latencyMs: 0 }), { mock: true });

describe("fixtures", () => {
  it("are all marked fictional", () => {
    for (const f of [deskFixture, simFixture, transcriptFixture]) expect((f as { fictional: boolean }).fictional).toBe(true);
  });

  it("every evidence ref in the reconcile fixtures resolves to a run and a transcript line", () => {
    const refs: string[] = [];
    const report = (deskFixture as unknown as { report: ReconcileReport }).report;
    for (const s of report.segments) for (const e of [...s.what_worked, ...s.what_failed]) refs.push(...e.refs);
    for (const i of report.red_lines) refs.push(...i.refs);
    for (const o of (deskFixture as unknown as { proposals: { evidence: { refs: string[] }[] }[] }).proposals) for (const e of o.evidence) refs.push(...e.refs);
    expect(refs.length).toBeGreaterThan(10);
    for (const r of refs) {
      const p = parseRef(r);
      expect(p, r).toBeDefined();
      const events: StreamEvent[] = transcriptFor(p!.runId);
      expect(p!.line, r).toBeLessThanOrEqual(events.length);
    }
  });

  it("verdict refs point at lines inside their transcript", async () => {
    const c = client();
    const runs = await c.call("runs.list", { tournament_id: "tones-2026-10-17" });
    const judged = runs.filter((r) => r.has_verdict).slice(0, 6);
    for (const r of judged) {
      const [events, verdict] = await Promise.all([c.call("run.transcript", { run_id: r.run_id }), c.call("run.verdict", { run_id: r.run_id })]);
      expect(events[0]).toMatchObject({ type: "stream_start", stream_id: r.run_id, tone_variant: r.spec.tone_variant });
      for (const line of annotationsByLine(verdict, r.run_id).keys()) expect(line).toBeLessThanOrEqual(events.length);
    }
  });
});

describe("mock daemon", () => {
  it("returns 8 pre-flight reports with PF-08 amber", async () => {
    const r = await client().call("preflight.run", {});
    expect(r.map((x) => x.check)).toEqual(["PF-01", "PF-02", "PF-03", "PF-04", "PF-05", "PF-06", "PF-07", "PF-08"]);
    expect(r.find((x) => x.check === "PF-08")).toMatchObject({ state: "amber" });
  });

  it("adds and deactivates directives, and plan.choose carries only active ones", async () => {
    const c = client();
    const added = await c.call("directive.add", { kind: "try", text: "Cold open with the Inspector", scope: "SEG-01" });
    expect(added).toMatchObject({ kind: "try", scope: "SEG-01", active: true });
    await c.call("directive.deactivate", { id: "dir-1" });
    const list: Directive[] = await c.call("directive.list", {});
    expect(list.find((d) => d.id === "dir-1")?.active).toBe(false);

    const { report_id } = await c.call("reconcile.run", { source: { tournament_id: "tones-2026-10-17" } });
    const plan = await c.call("plan.choose", { report_id, option_ids: ["opt-1", "opt-3"] });
    const ids = plan.directives.map((d) => d.id);
    expect(ids).toContain(added.id);
    expect(ids).not.toContain("dir-1");
    expect(plan.directives.every((d) => d.active)).toBe(true);
    expect(plan.lord_dispatch.length).toBeGreaterThan(50);
  });

  it("lord.chat turns a prevent instruction into a directive", async () => {
    const r = await client().call("lord.chat", { message: "Never mock a viewer's accent in SEG-01" });
    expect(r.directives_added).toHaveLength(1);
    expect(r.directives_added[0]).toMatchObject({ kind: "prevent", scope: "SEG-01", active: true });
  });

  it("reports unknown methods and bad params as rpc errors", async () => {
    const c = client();
    await expect(c.call("plan.choose", { report_id: "x", option_ids: [] })).rejects.toMatchObject({ kind: "rpc", code: "bad_params" });
    await expect(c.transport.call("nope.nope", {})).rejects.toBeInstanceOf(RpcError);
  });
});
