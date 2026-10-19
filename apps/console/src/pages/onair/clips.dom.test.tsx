// On Air · Clips in jsdom over the data-layer doubles: the funnel and rows from `clips.list`,
// one clip's gate checklist from `clips.get`, and the owner-act rules (signature required,
// no approval over a failed check, an override reason for an unmeasured one, publish only
// once approved).
import type { ClipCandidate, ClipsPage, GateCheck } from "@shadowrunr/contract";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, ok, role, roles, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";

const ID = "clip-00000000000000aa";

const PAGE: ClipsPage = {
  items: [
    { id: ID, status: "pending_owner_gate", score: 0.712, rank: 1, lane: "clip", segment: "SEG-02A", title: "HEX-05 annexed as REP", run_id: "r1", tournament_id: "t1", duration_s: 52.4, held: true, requires_mandatory_gate: true, render: "poster", created_ms: 1 },
    { id: "clip-00000000000000bb", status: "approved_for_upload", score: 0.5, rank: 2, lane: "review", segment: "SEG-01", title: "Malakor answers the-inspector", run_id: "r1", duration_s: 30, held: false, requires_mandatory_gate: false, created_ms: 1 },
  ],
  counts: { pending_owner_gate: 1, approved_for_upload: 1 },
  held: 1,
  total: 2,
};

const check = (id: string, state: GateCheck["state"], mandatory = false): GateCheck => ({ id, state, detail: `${id} detail`, ...(mandatory ? { mandatory } : {}) });

function clip(checks: GateCheck[], status: ClipCandidate["status"] = "pending_owner_gate"): ClipCandidate {
  const w = { marker: 0.35, chat: 0.25, audio: 0.15, hook: 0.25 };
  return {
    id: ID,
    created_ms: 1,
    status,
    score: 0.712,
    rank: 1,
    lane: "clip",
    segment: "SEG-02A",
    title: "HEX-05 annexed as REP",
    source: { run_id: "r1", tournament_id: "t1", stream_id: "s1", transcript: "sim/t1/r1/transcript.jsonl", line_from: 10, line_to: 40 },
    window: { anchor_ms: 60_000, start_ms: 50_000, end_ms: 70_000, spoken_start_ms: 1000, spoken_end_ms: 53_400, duration_s: 52.4, driver: "marker", lag_ms: 0, preroll_ms: 12_000, band_s: [45, 60], style: "seg02a-alleged" },
    markers: [
      { kind: "hex_reveal", ts_ms: 60_000, label: "HEX-05 revealed (REP)", admitted: true },
      { kind: "mark_moment", ts_ms: 61_000, label: "again", admitted: false, capped: "cooldown" },
    ],
    signals: { marker: 0.8, chat: 0.4, chat_z: 1.2, hook: 0.7, unmeasured: { audio: "no programme audio in the media-less phase" } },
    scoring: { nominal: w, effective: { marker: 0.412, chat: 0.294, audio: 0, hook: 0.294 }, provenance: "unmeasured: design hypothesis", agreeing: ["marker", "hook"] },
    linked_case_id: "DEMO",
    evidence: { case_id: "DEMO", case_title: "Demo (FICTIONAL)", fictional: true, hexes: [{ id: "HEX-05", label: "Promised yield", grade: "REP", sources: ["https://news.example/x"] }], worst_grade: "REP", citation: ["https://news.example/x"] },
    captions: [{ start_ms: 0, end_ms: 3000, speaker: "malakor", text: "Let the deficit be recorded." }],
    render: { kind: "poster", width: 1080, height: 1920, duration_s: 52.4, files: [{ path: "poster.svg", kind: "poster", bytes: 10 }], overlay: { alleged_header: true, grade_badge: "REP", citation: ["https://news.example/x"], ai_disclosure: true, captions: "transcript" }, fallback_reason: "recording tools missing", rendered_ms: 1 },
    gate: { requires_mandatory_gate: true, checks, held: checks.some((k) => k.state === "fail" || k.state === "unmeasured"), evaluated_ms: 1 },
    history: [{ status: "rendered_draft", at_ms: 1, by: "render" }],
  };
}

const PASSING = [check("care_state", "pass"), check("personal_data", "pass"), check("evidence_badge_burnt", "pass", true)];

let dispose: (() => void) | null = null;
afterEach(() => {
  dispose?.();
  dispose = null;
  cleanup();
});

function mount(hash: string, script: Script) {
  const m = mountPage(<Layer2 sub={hash.replace(/^#\/onair\//, "").split("/")} />, script, hash);
  dispose = m.dispose;
  return m;
}

const approveButton = () => screen.getByRole("button", { name: "Approve for upload" });

describe("On Air · Clips", () => {
  it("lists the funnel counts and the candidates best first", async () => {
    const m = mount("#/onair/clips", (method) => (method === "clips.list" ? ok(PAGE) : ok(null)));
    await settle();
    expect(m.calls).toContain("clips.list");
    expect(role("onair-clips-funnel")?.textContent).toContain("Owner gate");
    expect(role("onair-clips-funnel")?.textContent).toContain("weights unmeasured");
    const rows = roles("onair-clip-row");
    expect(rows).toHaveLength(2);
    expect(rows[0]?.textContent).toContain("0.712");
    expect(rows[0]?.textContent).toContain("alleged");
    expect(rows[0]?.getAttribute("href")).toBe(`#/onair/clips/${ID}`);
  });

  it("shows the gate checklist and keeps approve shut without a signature", async () => {
    mount(`#/onair/clips/${ID}`, (method) => (method === "clips.get" ? ok(clip(PASSING)) : ok(null)));
    await settle();
    const gate = role("onair-clip-gate");
    expect(gate?.querySelectorAll("li")).toHaveLength(3);
    expect(gate?.textContent).toContain("checklist");
    expect(role("onair-clip-signals")?.textContent).toContain("unmeasured");
    expect(approveButton()).toHaveProperty("disabled", true);
    await act(async () => {
      fireEvent.change(screen.getByPlaceholderText("your name"), { target: { value: "Owner" } });
    });
    expect(approveButton()).toHaveProperty("disabled", false);
    expect(role("onair-clip-override")).toBeNull();
  });

  it("refuses approval over a failed check, whatever the signature", async () => {
    mount(`#/onair/clips/${ID}`, (method) => (method === "clips.get" ? ok(clip([...PASSING, check("brigade_call", "fail")])) : ok(null)));
    await settle();
    expect(screen.getByText(/Held by brigade_call/)).toBeTruthy();
    expect(screen.getByPlaceholderText("your name")).toHaveProperty("disabled", true);
    expect(approveButton()).toHaveProperty("disabled", true);
  });

  it("asks for an override reason when a check is unmeasured", async () => {
    mount(`#/onair/clips/${ID}`, (method) => (method === "clips.get" ? ok(clip([...PASSING, check("no_open_correction", "unmeasured", true)])) : ok(null)));
    await settle();
    expect(role("onair-clip-override")?.textContent).toContain("no_open_correction");
    await act(async () => {
      fireEvent.change(screen.getByPlaceholderText("your name"), { target: { value: "Owner" } });
    });
    expect(approveButton()).toHaveProperty("disabled", true);
    await act(async () => {
      fireEvent.change(screen.getByPlaceholderText("why approving without this check is sound"), { target: { value: "fictional demo case" } });
    });
    expect(approveButton()).toHaveProperty("disabled", false);
  });

  it("publishes only an approved clip, and says it is a dry run", async () => {
    mount(`#/onair/clips/${ID}`, (method) => (method === "clips.get" ? ok(clip(PASSING)) : ok(null)));
    await settle();
    expect(role("onair-clip-publish")).toHaveProperty("disabled", true);
    cleanup();
    dispose?.();
    const m = mount(`#/onair/clips/${ID}`, (method) =>
      method === "clips.get"
        ? ok(clip(PASSING, "approved_for_upload"))
        : method === "clips.publish"
          ? ok({ id: ID, dry_run: true, requests: [{ platform: "youtube", at_ms: 1, file: "requests/youtube.json", applicable: false, budget_left: 100, note: "resumable upload" }] })
          : ok(null),
    );
    await settle();
    const publish = role("onair-clip-publish");
    expect(publish).toHaveProperty("disabled", false);
    await act(async () => {
      fireEvent.click(publish as Element);
    });
    await settle();
    expect(m.calls).toContain("clips.publish");
    expect(role("onair-clip-dry-runs")?.textContent).toContain("DRY RUN — nothing is sent");
  });
});
