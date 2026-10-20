// On Air's clips pager is ui/Button.
import { act, cleanup, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { mountPage, ok, settle } from "../../kit/testing";
import { buttonClass } from "../../ui/Button";
import { Layer2 } from "./Layer2";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
});

describe("On Air · Clips on ui/Button", () => {
  it("pages through secondary Buttons, both shut on a single page", async () => {
    const item = { id: "clip-00000000000000bb", status: "approved_for_upload", score: 0.5, rank: 1, lane: "review", segment: "SEG-01", title: "Malakor answers the-inspector", run_id: "r1", duration_s: 30, held: false, requires_mandatory_gate: false, created_ms: 1 };
    const page = { items: [item], counts: { approved_for_upload: 1 }, held: 0, total: 1 };
    const m = mountPage(<Layer2 sub={["clips"]} />, (method) => (method === "clips.list" ? ok(page) : ok([])), "#/onair/clips");
    dispose = m.dispose;
    await settle();
    await act(async () => undefined);
    for (const name of ["Previous", "Next"]) {
      const b = screen.getByRole("button", { name });
      expect(b.className).toBe(buttonClass("secondary"));
      expect((b as HTMLButtonElement).disabled).toBe(true);
    }
  });
});
