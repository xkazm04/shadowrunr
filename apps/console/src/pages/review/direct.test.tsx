// The player's Direct actions on the data layer's doubles: "Direct this segment" opens the
// segment's directions with the line quoted at the end of what the file holds, "Direct
// behaviour" does the same into directions.md, and nothing is written until Save.
import { act, cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ReviewSession } from "@shadowrunr/contract";
import { mountPage, ok, settle } from "../../kit/testing";
import { derive } from "./derive";
import { Layer2 } from "./Layer2";
import { reviewSession, SESSION } from "./tabs.fixture";

let dispose: (() => void) | null = null;
afterEach(() => {
  cleanup();
  dispose?.();
  dispose = null;
});

async function mount(rs: ReviewSession) {
  const sent: { method: string; body: string }[] = [];
  const m = mountPage(
    <Layer2 sub={[SESSION, "player", "roast"]} />,
    (method, body) => {
      sent.push({ method, body });
      if (method === "review.session") return ok(rs);
      if (method === "review.sessions" || method === "review.feedback.list") return ok([]);
      if (method === "segments.directions.get") return ok({ character: "malakor", segment_id: "SEG-01", text: "- Keep the room moving.\n", updated_ms: 5 });
      if (method === "segments.directions.set") return ok({ character: "malakor", segment_id: "SEG-01", text: SAVED, updated_ms: 6 });
      if (method === "tone.directions.get") return ok({ character: "malakor", file: "directions.md", text: "" });
      return { status: 404, body: { error: { code: "method_not_found", message: method } } };
    },
    `#/review/${SESSION}/player/roast`,
  );
  dispose = m.dispose;
  await settle(8);
  return sent;
}

const SAVED = "- Keep the room moving.\n- Roast the chat, not the viewer.\n";
const field = () => document.querySelector<HTMLTextAreaElement>("[data-role='review-direct-text']");
const writes = (sent: { method: string; body: string }[]) => sent.filter((s) => s.method.endsWith(".set"));

describe("directing from the review player", () => {
  it("quotes the line into the segment's directions and writes only on Save", async () => {
    const rs = reviewSession();
    const first = derive(rs).says.find((s) => s.kind === "roast");
    if (!first) throw new Error("the fixture has a roast line");
    const sent = await mount(rs);
    expect(document.querySelector("[data-role='review-direct']")?.textContent).toBe("Direct this segmentDirect behaviour");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Direct this segment" })));
    await settle(6);
    const quote = `> L${first.line} "${first.text.replace(/\s+/g, " ").trim()}"`;
    expect(field()?.value).toBe(`- Keep the room moving.\n${quote}\n`);
    expect(sent.find((s) => s.method === "segments.directions.get")?.body).toContain('"segment_id":"SEG-01"');
    expect(document.querySelector("[data-role='review-direct-drawer']")?.textContent).toContain("segments/SEG-01.md");
    expect(writes(sent)).toHaveLength(0);

    await act(async () => fireEvent.change(field() as HTMLTextAreaElement, { target: { value: SAVED } }));
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save segment directions" })));
    await settle(6);
    const w = writes(sent);
    expect(w.map((s) => s.method)).toEqual(["segments.directions.set"]);
    expect(w[0]?.body).toContain(`"params":${JSON.stringify({ character: "malakor", segment_id: "SEG-01", text: SAVED })}`);
    expect(screen.getByText("Saved")).toBeTruthy();
  });

  it("directs behaviour into directions.md, the quote alone on an empty file", async () => {
    const rs = reviewSession();
    const first = derive(rs).says.find((s) => s.kind === "roast");
    const sent = await mount(rs);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Direct behaviour" })));
    await settle(6);
    expect(field()?.value).toBe(`> L${first?.line} "${first?.text.replace(/\s+/g, " ").trim()}"\n`);
    expect(sent.some((s) => s.method === "tone.directions.get")).toBe(true);
    expect(writes(sent)).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Save directions" }).hasAttribute("disabled")).toBe(false);
  });
});
