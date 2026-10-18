// On Air's two P7 hooks in jsdom: the transport's "Pre-flight red" link (read from the one
// `preflight.run` query, never fetched twice) and "Open in stage" in the run detail, with the
// opener mocked.
import type { HealthReport, StreamEvent } from "@shadowrunr/contract";
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FetchReply } from "../../data/testing";
import { mountPage } from "../../kit/testing";
import { Layer1, redChecks } from "./Layer1";
import { Layer2 } from "./Layer2";
import { TABLES, runRow, transcript } from "./testdata";

const openExternal = vi.hoisted(() => vi.fn((_url: string) => Promise.resolve()));
vi.mock("../../kit/openExternal", () => ({ openExternal, inTauri: () => false }));

const GREEN: HealthReport[] = [
  { check: "PF-01", state: "green", detail: "ok" },
  { check: "PF-08", state: "amber", detail: "not in media-less phase (TTS, avatar, OBS, chat connectors)" },
];
const RED: HealthReport[] = [...GREEN, { check: "PF-03", state: "red", detail: "memory store not writable" }];

function daemon(preflight: HealthReport[] | null, events: StreamEvent[] = transcript(60)) {
  return (method: string): FetchReply => {
    switch (method) {
      case "runs.page":
        return { body: { result: { items: [runRow(0), runRow(1)] } } };
      case "run.transcript":
        return { body: { result: events } };
      case "segments.tables":
        return { body: { result: TABLES } };
      case "preflight.run":
        return preflight === null ? { network: true } : { body: { result: preflight } };
      case "directive.list":
      case "reconcile.list":
        return { body: { result: [] } };
      default:
        return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    }
  };
}

let dispose: (() => void) | null = null;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
  openExternal.mockClear();
});

async function settle(rounds = 8) {
  for (let i = 0; i < rounds; i++) await act(async () => new Promise((r) => setTimeout(r, 0)));
}

const link = () => document.querySelector("[data-role='onair-preflight-red']");

describe("On Air transport: Pre-flight red", () => {
  it("shows the red link to Brain's board when the latest pre-flight has a red check", async () => {
    const m = mountPage(<Layer1 active />, daemon(RED), "#/onair");
    dispose = m.dispose;
    await settle();
    expect(link()?.textContent).toBe("Pre-flight red");
    expect(link()?.getAttribute("href")).toBe("#/brain/preflight");
    expect(link()?.getAttribute("title")).toContain("PF-03");
    expect(document.querySelector("[data-role='onair-transport']")?.contains(link())).toBe(true);
    expect(m.calls.filter((c) => c === "preflight.run")).toHaveLength(1);
  });

  it("stays quiet when nothing is red (the expected media amber included) or the check cannot be read", async () => {
    let m = mountPage(<Layer1 active />, daemon(GREEN), "#/onair");
    await settle();
    expect(link()).toBeNull();
    m.dispose();
    cleanup();
    m = mountPage(<Layer1 active />, daemon(null), "#/onair");
    dispose = m.dispose;
    await settle();
    expect(link()).toBeNull();
  });

  it("does not read the pre-flight while hidden", async () => {
    const m = mountPage(<Layer1 active={false} />, daemon(RED), "#/onair");
    dispose = m.dispose;
    await settle();
    expect(m.calls).not.toContain("preflight.run");
  });

  it("lists red checks by id", () => {
    expect(redChecks(RED)).toEqual(["PF-03"]);
    expect(redChecks(GREEN)).toEqual([]);
    expect(redChecks(undefined)).toEqual([]);
  });
});

describe("On Air run detail: Open in stage", () => {
  it("opens the stage's replay of the tournament-qualified run through the opener", async () => {
    const m = mountPage(<Layer2 sub={["rundown", "t-0", "run-0"]} />, daemon(GREEN), "#/onair/rundown/t-0/run-0");
    dispose = m.dispose;
    await settle();
    const button = screen.getByRole("button", { name: "Open in stage" });
    expect(button.getAttribute("data-role")).toBe("onair-open-stage");
    await act(async () => fireEvent.click(button));
    expect(openExternal).toHaveBeenCalledTimes(1);
    expect(openExternal).toHaveBeenCalledWith("http://localhost:5178/?run=t-0%2Frun-0");
  });
});
