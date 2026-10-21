// On Air's run detail links the Brain transcript with the tournament-qualified run: run ids are
// deterministic per matrix cell, so a bare id repeats across tournaments and the daemon refuses it.
import type { StreamEvent } from "@shadowrunr/contract";
import { cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { FetchReply } from "../../data/testing";
import { mountPage } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { runRow, transcript } from "./testdata";

let dispose: (() => void) | null = null;

function daemon(events: StreamEvent[]) {
  return (method: string): FetchReply => {
    switch (method) {
      case "runs.page":
        return { body: { result: { items: [runRow(0, "t2")] } } };
      case "run.transcript":
        return { body: { result: events } };
      default:
        return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    }
  };
}

afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

describe("On Air run detail -> Brain transcript", () => {
  it("links the transcript and every beat's line with the qualified t2/r7, not the bare r7", async () => {
    dispose = mountPage(<Layer2 sub={["rundown", "t2", "r7"]} />, daemon(transcript(40)), "#/onair/rundown/t2/r7").dispose;
    const read = await screen.findByText("Read the transcript", undefined, { timeout: 10_000 });
    expect(read.closest("a")?.getAttribute("href")).toBe("#/brain/transcripts/t2%2Fr7");
    await screen.findByText(/Beats ·/);
    const refs = [...document.querySelectorAll("[role='list'][aria-label='Beats'] a")].map((a) => a.getAttribute("href") ?? "");
    expect(refs.length).toBeGreaterThan(0);
    for (const href of refs) expect(href).toMatch(/^#\/brain\/transcripts\/t2%2Fr7\/\d+$/);
  });
});
