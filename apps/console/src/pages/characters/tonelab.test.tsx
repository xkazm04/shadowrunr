// The Tone Lab tab in jsdom against a scripted daemon that keeps a bank and directions in memory:
// the bank's filters and status edits, variants approved as edited, a Review line arriving by
// route, the inbox, the directions editor, every async state and no enum id on screen.
import { act, cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { LineSituation, LineStatus, ToneLine, ToneReviewItem } from "@shadowrunr/contract";
import { fail, mountPage, ok, param, role, roles, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { malakorDetail } from "./testgen";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
});

const SID = "sbx-20261020-120000-5m";

const item = (o: Partial<ToneReviewItem> = {}): ToneReviewItem => ({
  session_id: SID,
  line: 4,
  text: "Moth, you are a rounding error.",
  tone: "theatrical",
  tags: [],
  ...o,
});

interface Daemon {
  lines: ToneLine[];
  directions: { text: string };
  /** Every request body, as sent. */
  sent: { method: string; raw: string }[];
  variants?: () => ReturnType<Script>;
  list?: () => ReturnType<Script>;
  inbox?: { candidates: ToneReviewItem[]; suggestions: ToneReviewItem[] };
  review?: ToneReviewItem;
}

function daemon(o: Partial<Daemon> = {}): Daemon & { script: Script } {
  const d: Daemon = {
    lines: [
      { id: "th-01", status: "seed", tags: ["roast_request"], text: "A seed roast." },
      { id: "th-02", status: "approved", tags: ["bait"], text: "An approved bait line." },
      { id: "th-03", status: "retired", tags: ["general"], text: "A retired line." },
    ],
    directions: { text: "# Directions\n\n## Do\n\n- Name the target.\n" },
    sent: [],
    ...o,
  };
  const script: Script = (method, raw) => {
    d.sent.push({ method, raw });
    const file = `lines/${param(raw, "tone") ?? ""}.toml`;
    switch (method) {
      case "character.get":
        return ok(malakorDetail());
      case "tone.lines.list":
        return d.list ? d.list() : ok({ character: "malakor", tone: param(raw, "tone"), file, lines: d.lines });
      case "tone.lines.set_status": {
        const l = d.lines.find((x) => x.id === param(raw, "id"));
        if (!l) return fail("not_found", "no line");
        l.status = param(raw, "status") as LineStatus;
        return ok({ line: l, file });
      }
      case "tone.lines.add": {
        const tags = (/"tags":\[([^\]]*)\]/.exec(raw)?.[1] ?? "").split(",").map((t) => t.replace(/"/g, "")) as LineSituation[];
        const line: ToneLine = { id: `th-${String(d.lines.length + 1).padStart(2, "0")}`, status: "approved", tags, text: param(raw, "text") ?? "" };
        d.lines.push(line);
        return ok({ line, file });
      }
      case "tone.variants":
        return d.variants ? d.variants() : ok({ candidates: ["Candidate one.", "Candidate two."] });
      case "tone.directions.get":
        return ok({ character: "malakor", file: "directions.md", text: d.directions.text });
      case "tone.directions.set":
        // The body escapes newlines; the panel sends what the text area holds.
        d.directions.text = (param(raw, "text") ?? "").replace(/\\n/g, "\n");
        return ok({ character: "malakor", file: "directions.md", text: d.directions.text });
      case "tone.inbox":
        return ok(d.inbox ?? { candidates: [], suggestions: [] });
      case "tone.review_line":
        return d.review ? ok(d.review) : fail("not_found", "no session");
      default:
        return fail("method_not_found", method);
    }
  };
  return { ...d, script };
}

async function mount(d: ReturnType<typeof daemon>, ...more: string[]) {
  const sub = ["malakor", "package", "tone-lab", ...more];
  const m = mountPage(<Layer2 sub={sub} />, d.script, `#/characters/${sub.join("/")}`);
  dispose = m.dispose;
  await settle(8);
  return m;
}

const click = (el: Element | null | undefined) => act(async () => fireEvent.click(el ?? document.body));
const bankLines = () => roles("tone-lab-bank-line");
/** The params of each call to `method`, as the JSON the console sent. */
const sentOf = (d: Daemon, method: string) => d.sent.filter((s) => s.method === method).map((s) => s.raw.slice(s.raw.indexOf('"params":') + 9, -1));
const json = (v: unknown): string => JSON.stringify(v);

describe("characters/<id>/package/tone-lab", () => {
  it("lists the bank, filters it, approves a seed and retires a line", async () => {
    const d = daemon();
    await mount(d);
    expect(role("tone-lab-commit")?.textContent).toContain("stay uncommitted until you commit them");
    expect(bankLines()).toHaveLength(3);
    const bank = role("tone-lab-bank") as HTMLElement;
    await click(within(bank).getByRole("button", { name: /^Approved/ }));
    expect(bankLines().map((l) => l.getAttribute("data-line-id"))).toEqual(["th-02"]);
    await click(within(bank).getByRole("button", { name: /^All/ }));
    await act(async () => fireEvent.change(role("tone-lab-bank-tag") as HTMLSelectElement, { target: { value: "roast_request" } }));
    expect(bankLines().map((l) => l.getAttribute("data-line-id"))).toEqual(["th-01"]);
    await click(within(bankLines()[0] as HTMLElement).getByRole("button", { name: "Approve" }));
    await settle();
    expect(sentOf(d, "tone.lines.set_status")).toEqual([json({ character: "malakor", tone: "theatrical", id: "th-01", status: "approved" })]);
    expect(bankLines()[0]?.getAttribute("data-status")).toBe("approved");
    expect(role("tone-lab-commit")?.textContent).toContain("Changed here: lines/theatrical.toml");
    await act(async () => fireEvent.change(role("tone-lab-bank-tag") as HTMLSelectElement, { target: { value: "all" } }));
    await click(within(bankLines()[1] as HTMLElement).getByRole("button", { name: "Retire" }));
    await settle();
    expect(d.lines[1]?.status).toBe("retired");
  });

  it("edits a bank line as a new approved line with its situations", async () => {
    const d = daemon();
    await mount(d);
    const row = bankLines()[1] as HTMLElement;
    await click(within(row).getByRole("button", { name: "Edit as new" }));
    await act(async () => fireEvent.change(within(row).getByRole("textbox"), { target: { value: "A sharper bait line." } }));
    await click(within(row).getByRole("button", { name: "Save as a new line" }));
    await settle();
    expect(sentOf(d, "tone.lines.add")).toEqual([json({ character: "malakor", tone: "theatrical", text: "A sharper bait line.", tags: ["bait"], status: "approved" })]);
    expect(bankLines()).toHaveLength(4);
  });

  it("drafts variants for a situation and approves one as edited", async () => {
    const d = daemon();
    await mount(d);
    await act(async () => fireEvent.change(role("tone-lab-situation") as HTMLSelectElement, { target: { value: "vote_loss" } }));
    await act(async () => fireEvent.change(role("tone-lab-note") as HTMLTextAreaElement, { target: { value: "Name the vote." } }));
    await click(role("tone-lab-generate"));
    await settle();
    expect(sentOf(d, "tone.variants")).toEqual([json({ character: "malakor", tone: "theatrical", situation: "vote_loss", n: 3, note: "Name the vote." })]);
    const cands = role("tone-lab-candidates") as HTMLElement;
    await act(async () => fireEvent.change(within(cands).getByRole("textbox", { name: "Candidate 2" }), { target: { value: "Candidate two, sharper." } }));
    await click(within(cands).getByRole("button", { name: "Approve as edited" }));
    await settle();
    expect(sentOf(d, "tone.lines.add")).toEqual([json({ character: "malakor", tone: "theatrical", text: "Candidate two, sharper.", tags: ["vote_loss"], status: "approved" })]);
    expect(cands.textContent).toContain("Approved as th-04");
    expect(within(cands).getAllByRole("button", { name: "Approve" })).toHaveLength(1);
  });

  it("prefills the variants from a Review line sent by route", async () => {
    const d = daemon({ review: item({ tone: "unhinged-roastmaster", situation: "bait", note: "Meaner.", tags: ["flat"] }) });
    await mount(d, SID, "4");
    expect(sentOf(d, "tone.review_line")).toEqual([json({ character: "malakor", session_id: SID, line: 4 })]);
    expect((role("tone-lab-line") as HTMLTextAreaElement).value).toBe("Moth, you are a rounding error.");
    expect((role("tone-lab-note") as HTMLTextAreaElement).value).toBe("Meaner.");
    expect((role("tone-lab-situation") as HTMLSelectElement).value).toBe("bait");
    expect(screen.getByRole("button", { name: "Unhinged roastmaster" }).getAttribute("aria-pressed")).toBe("true");
    expect(document.body.textContent).toContain("from session 20261020-120000-5m, line 4");
    expect(sentOf(d, "tone.lines.list").at(-1)).toBe(json({ character: "malakor", tone: "unhinged-roastmaster" }));
  });

  it("says when the Review line does not arrive", async () => {
    await mount(daemon(), SID, "4");
    expect(document.body.textContent).toContain("The Review line did not arrive");
  });

  it("adds an inbox candidate to its tone's bank and appends a suggestion to the directions", async () => {
    const d = daemon({
      inbox: {
        candidates: [item({ tags: ["great_roast"], situation: "roast_request", at_ms: 2 })],
        suggestions: [item({ line: 6, text: "Vote now, worms.", tags: ["flat"], note: "Say why the vote matters.", at_ms: 1 })],
      },
    });
    await mount(d);
    const inbox = role("tone-lab-inbox") as HTMLElement;
    expect(inbox.textContent).toContain("Great roast");
    expect(inbox.textContent).toContain("Asked to be roasted");
    await click(within(inbox).getByRole("button", { name: "Add to the Theatrical bank" }));
    await settle();
    expect(sentOf(d, "tone.lines.add")).toEqual([json({ character: "malakor", tone: "theatrical", text: "Moth, you are a rounding error.", tags: ["roast_request"], status: "approved" })]);
    await click(within(inbox).getByRole("button", { name: "Add to directions" }));
    const area = role("tone-lab-directions-text") as HTMLTextAreaElement;
    expect(area.value).toBe("# Directions\n\n## Do\n\n- Name the target.\n- Say why the vote matters.\n");
    await click(role("tone-lab-directions-save"));
    await settle();
    expect(d.directions.text).toBe(area.value);
    expect(role("tone-lab-directions")?.textContent).toContain("Saved");
    expect(role("tone-lab-commit")?.textContent).toContain("lines/theatrical.toml, directions.md");
  });

  it("shows the empty, error and model states", async () => {
    const d = daemon({ lines: [], variants: () => ok({ candidates: [], gated: 2 }) });
    await mount(d);
    expect(role("tone-lab-bank")?.textContent).toContain("No lines in this tone's bank yet");
    expect(role("tone-lab-inbox")?.textContent).toContain("Nothing from Review yet");
    await click(role("tone-lab-generate"));
    await settle();
    expect(role("tone-lab-variants")?.textContent).toContain("The model gave no new line");
    expect(role("tone-lab-variants")?.textContent).toContain("2 would have tripped the output gate");
    cleanup();
    dispose?.();
    await mount(daemon({ list: () => fail("io", "disk gone"), variants: () => fail("engine", "fleet down") }));
    expect(role("tone-lab-bank")?.querySelector("[data-state]")?.getAttribute("data-state")).not.toBe("loading");
    expect(role("tone-lab-bank")?.textContent).toContain("disk gone");
    await click(role("tone-lab-generate"));
    await settle();
    expect(role("tone-lab-variants")?.textContent).toContain("No variants");
    expect(role("tone-lab-variants")?.textContent).toContain("fleet down");
  });

  it("renders no enum id", async () => {
    await mount(
      daemon({
        inbox: { candidates: [item({ tags: ["great_roast", "too_mean"], situation: "research_wait" })], suggestions: [] },
      }),
    );
    const text = document.body.textContent ?? "";
    for (const id of ["roast_request", "research_wait", "great_roast", "too_mean", "vote_loss", "ai_question", "unhinged-roastmaster"]) expect(text).not.toContain(id);
  });
});
