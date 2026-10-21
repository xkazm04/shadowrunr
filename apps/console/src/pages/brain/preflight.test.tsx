// brain/preflight in jsdom on the real data layer: a scripted `preflight.run` with mixed states
// (the media check's expected amber included), the detail opened in place, a re-run by the key
// command and by the button, the poster lamp, the designed states, and "Open in stage" from the
// transcript viewer with the opener mocked.
import type { HealthReport } from "@shadowrunr/contract";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emitKeyCommand } from "../../kit/keyCommands";
import { fail, mountPage as mount, ok, offline, role, roles, settle, type Script } from "../../kit/testing";
import { Layer1 } from "./Layer1";
import { Layer2 } from "./Layer2";
import { isExpected, worstState } from "./preflightModel";
import { daemon, Recorder, season, transcript } from "./testgen";

const openExternal = vi.hoisted(() => vi.fn((_url: string) => Promise.resolve()));
vi.mock("../../kit/openExternal", () => ({ openExternal, inTauri: () => false }));

const MEDIA = "not in media-less phase (TTS, avatar, OBS, chat connectors)";

const FIRST: HealthReport[] = [
  { check: "PF-01", state: "green", detail: "claude_cli: ok" },
  { check: "PF-02", state: "green", detail: "malakor loaded" },
  { check: "PF-03", state: "red", detail: "memory store not writable: access denied" },
  { check: "PF-04", state: "green", detail: "2 active segments" },
  { check: "PF-05", state: "amber", detail: "placeholder only" },
  { check: "PF-06", state: "green", detail: "calibration loaded" },
  { check: "PF-07", state: "green", detail: "runs writable" },
  { check: "PF-08", state: "amber", detail: MEDIA },
  { check: "PF-09", state: "green", detail: "personas reachable" },
];
/** The second run: the memory store recovered. */
const SECOND = FIRST.map((r) => (r.check === "PF-03" ? { ...r, state: "green" as const, detail: "memory store writable" } : r));

/** A daemon whose preflight answers FIRST, then SECOND on every later run. */
function preflightDaemon(base: Script = () => fail("method_not_found", "nope")): Script & { runs: () => number } {
  let n = 0;
  const script = ((method: string, sent: string) => {
    if (method !== "preflight.run") return base(method, sent);
    n += 1;
    return ok(n === 1 ? FIRST : SECOND);
  }) as Script & { runs: () => number };
  script.runs = () => n;
  return script;
}

const row = (check: string) => document.querySelector(`[data-role='brain-preflight-row'][data-check='${check}']`);

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  openExternal.mockClear();
});

describe("brain/preflight", () => {
  it("renders the PF-01..PF-09 board with mixed states and the media amber as expected", async () => {
    mount(<Layer2 sub={["preflight"]} />, preflightDaemon(), "#/brain/preflight");
    await settle(8);
    expect(roles("brain-preflight-row")).toHaveLength(9);
    expect(row("PF-03")?.getAttribute("data-state")).toBe("red");
    expect(row("PF-05")?.getAttribute("data-state")).toBe("amber");
    expect(row("PF-08")?.getAttribute("data-state")).toBe("expected");
    expect(row("PF-08")?.textContent).toContain("Media stack");
    expect(row("PF-08")?.textContent).toContain("amber · expected");
    expect(role("brain-preflight")?.getAttribute("data-worst")).toBe("red");
    expect(role("brain-preflight-verdict")?.textContent).toMatch(/^Red: a stream start is refused/);
    expect(screen.getByText("6 green · 1 amber · 1 red · 1 expected")).toBeTruthy();
    expect(role("brain-preflight-last")?.textContent).toMatch(/^Last run \d\d:\d\d:\d\d$/);
  });

  it("opens a check's detail in place, on demand", async () => {
    mount(<Layer2 sub={["preflight"]} />, preflightDaemon(), "#/brain/preflight");
    await settle(8);
    expect(roles("brain-preflight-detail")).toHaveLength(0);
    const head = row("PF-03")?.querySelector("button");
    expect(head?.getAttribute("aria-expanded")).toBe("false");
    await act(async () => head && fireEvent.click(head));
    expect(head?.getAttribute("aria-expanded")).toBe("true");
    expect(row("PF-03")?.querySelector("[data-role='brain-preflight-detail']")?.textContent).toContain("access denied");
    const media = row("PF-08")?.querySelector("button");
    await act(async () => media && fireEvent.click(media));
    expect(row("PF-08")?.textContent).toContain("Expected in the media-less phase: not a fault.");
    await act(async () => head && fireEvent.click(head));
    expect(row("PF-03")?.querySelector("[data-role='brain-preflight-detail']")).toBeNull();
  });

  it("re-runs on the key command (what the keymap's `r` emits) and on the button", async () => {
    const script = preflightDaemon();
    mount(<Layer2 sub={["preflight"]} />, script, "#/brain/preflight");
    await settle(8);
    expect(script.runs()).toBe(1);
    let handled = false;
    await act(async () => {
      handled = emitKeyCommand("rerun");
    });
    expect(handled).toBe(true);
    await settle(8);
    expect(script.runs()).toBe(2);
    expect(row("PF-03")?.getAttribute("data-state")).toBe("green");
    expect(role("brain-preflight")?.getAttribute("data-worst")).toBe("amber");
    const button = role("brain-preflight-rerun");
    await act(async () => button && fireEvent.click(button));
    await settle(8);
    expect(script.runs()).toBe(3);
  });

  it("owns the key command only while the board is on screen", async () => {
    const { view } = mount(<Layer2 sub={["preflight"]} />, preflightDaemon(), "#/brain/preflight");
    await settle(8);
    view.unmount();
    expect(emitKeyCommand("rerun")).toBe(false);
  });

  it("shows the offline, empty and degraded states", async () => {
    mount(<Layer2 sub={["preflight"]} />, offline, "#/brain/preflight");
    await settle(8);
    expect(screen.getAllByText("The daemon is not reachable")).toHaveLength(1);
    expect(role("brain-preflight-last")?.textContent).toBe("Not run yet");
    cleanup();

    mount(<Layer2 sub={["preflight"]} />, (m) => (m === "preflight.run" ? ok([]) : fail("method_not_found", m)), "#/brain/preflight");
    await settle(8);
    expect(screen.getByText("The daemon reported no pre-flight checks")).toBeTruthy();
    cleanup();

    let n = 0;
    mount(<Layer2 sub={["preflight"]} />, (m) => (m !== "preflight.run" ? fail("method_not_found", m) : ++n === 1 ? ok(FIRST) : fail("io", "disk gone")), "#/brain/preflight");
    await settle(8);
    await act(async () => {
      emitKeyCommand("rerun");
    });
    await settle(8);
    expect(document.querySelector("[data-state='degraded']")?.textContent).toMatch(/Showing the reply from \d\d:\d\d:\d\d · /);
    expect(roles("brain-preflight-row")).toHaveLength(9);
  });

  it("reads the expected media amber as no fault, so an all-clear board stays green", () => {
    const clear = SECOND.filter((r) => r.check !== "PF-05");
    expect(clear.filter(isExpected).map((r) => r.check)).toEqual(["PF-08"]);
    expect(worstState(clear)).toBe("green");
    expect(worstState(SECOND)).toBe("amber");
    expect(worstState(FIRST)).toBe("red");
  });
});

describe("brain layer 1 pre-flight lamp", () => {
  beforeEach(() => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(() => new Recorder() as never);
  });

  it("lights the worst state on the poster and links to the board", async () => {
    const s = season({ tournaments: 1, runs: 4, rowsPerRun: 20, transcriptLines: 50 });
    mount(<Layer1 active />, preflightDaemon(daemon(s)), "#/brain");
    await settle(8);
    expect(role("brain-preflight-lamp")?.getAttribute("data-state")).toBe("red");
    const door = role("brain-door-preflight");
    expect(door?.getAttribute("href")).toBe("#/brain/preflight");
    expect(door?.textContent).toBe("Pre-flight · red");
  });

  it("reads unknown while the daemon is down", async () => {
    mount(<Layer1 active />, daemon(null), "#/brain");
    await settle(8);
    expect(role("brain-preflight-lamp")?.getAttribute("data-state")).toBe("unknown");
    expect(role("brain-door-preflight")?.textContent).toBe("Pre-flight · not read");
  });
});

describe("brain/transcripts/<run>: Open in stage", () => {
  it("opens the stage's replay of the run through the opener", async () => {
    const s = season({ tournaments: 1, runs: 1, rowsPerRun: 1, transcriptLines: 10 });
    mount(<Layer2 sub={["transcripts", "gen-t0000/r1"]} />, daemon(s, new Map([["gen-t0000/r1", transcript(40, "r1")]])), "#/brain/transcripts/gen-t0000%2Fr1");
    await settle(8);
    const button = role("brain-open-stage");
    expect(button?.textContent).toBe("Open in stage");
    await act(async () => button && fireEvent.click(button));
    expect(openExternal).toHaveBeenCalledTimes(1);
    expect(openExternal).toHaveBeenCalledWith("http://localhost:5178/?run=gen-t0000%2Fr1");
  });

  it("says so when the opener fails", async () => {
    openExternal.mockImplementationOnce(() => Promise.reject(new Error("denied by capability")));
    const s = season({ tournaments: 1, runs: 1, rowsPerRun: 1, transcriptLines: 10 });
    mount(<Layer2 sub={["transcripts", "r1"]} />, daemon(s, new Map([["r1", transcript(40, "r1")]])), "#/brain/transcripts/r1");
    await settle(8);
    const button = role("brain-open-stage");
    await act(async () => button && fireEvent.click(button));
    await settle(2);
    expect(screen.getByRole("alert").textContent).toBe("The stage did not open: denied by capability");
  });
});
