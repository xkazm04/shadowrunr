// The Tones tab's evidence: each tone's measured record from the package's one evidence source
// (useCharacterEvidence, shared with Situations), and Make active as a previewed switch.
import { act, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { fail, mountPage, ok, role, roles, settle, type Script } from "../../kit/testing";
import { useRoute } from "../../router";
import { tournamentPick } from "./evidence";
import { Layer2 } from "./Layer2";
import { pkgRoute } from "./routes";
import { genToneResults, malakorDetail, malakorSummary } from "./testgen";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
  tournamentPick.reset();
});

interface Daemon {
  results?: ReturnType<typeof genToneResults>;
  tournaments?: string[];
  /** Every character.tone.set request body, in order. */
  toneSent?: string[];
  /** The daemon's active.json tone, changed by character.tone.set. */
  tone?: { value: string };
}

const daemon =
  (o: Daemon = {}): Script =>
  (method, sent) => {
    switch (method) {
      case "character.get":
        return ok({ ...malakorDetail(), ...(o.tone ? { active_tone: o.tone.value } : {}) });
      case "character.tone.set": {
        o.toneSent?.push(sent);
        const tone = /"tone":"([^"]+)"/.exec(sent)?.[1] ?? "";
        if (o.tone) o.tone.value = tone;
        return ok(malakorSummary({ active_tone: tone }));
      }
      case "tournament.list":
        return ok((o.tournaments ?? ["t-pilot", "t-scale"]).map((id, i) => ({ tournament_id: id, runs: 4, completed: 4, instrument_hash: "h", character: "malakor", updated_ms: 1000 + i })));
      case "tournament.results":
        return ok(o.results ?? genToneResults("t-scale", {}));
      default:
        return fail("method_not_found", method);
    }
  };

function mount(sub: string[], o: Daemon = {}) {
  const m = mountPage(<Layer2 sub={sub} />, daemon(o), `#/characters/${sub.join("/")}`);
  dispose = m.dispose;
  return m;
}
/** Layer 2 at whatever route the router holds, so a test can move between tabs in one mount. */
function Routed() {
  const r = useRoute();
  return <Layer2 sub={r.view === "page" ? r.sub : []} />;
}

const toneCard = (label: string) => roles("characters-tone-card").find((c) => c.querySelector("h3")?.textContent === label);
const press = async (el: Element | null | undefined) => {
  await act(async () => {
    (el as HTMLButtonElement | null | undefined)?.click();
  });
  await settle();
};
const makeActive = (label: string) => press(toneCard(label)?.querySelector("[data-role='characters-tone-set']"));

describe("each tone shows its record", () => {
  it("reads 'Unmeasured in <tid>' for a tone with no cell, never zero counts", async () => {
    mount(["malakor", "package", "tones"], { results: genToneResults("t-scale", { theatrical: [{ judged: 12 }], "gleeful-chaos": [{ judged: 6 }] }) });
    await settle(6);
    const ev = toneCard("Weary middle manager")?.querySelector("[data-role='characters-tone-evidence']");
    expect(ev?.textContent).toContain("Unmeasured in t-scale");
    expect(ev?.textContent).not.toMatch(/\b0\b/);
  });

  it("marks a thin record (judged < 5) and leaves a solid one unmarked", async () => {
    mount(["malakor", "package", "tones"], { results: genToneResults("t-scale", { "gleeful-chaos": [{ judged: 3 }], "deadpan-bureaucrat": [{ judged: 12 }] }) });
    await settle(6);
    const thin = toneCard("Gleeful chaos")?.querySelector("[data-role='characters-tone-evidence']");
    const solid = toneCard("Deadpan bureaucrat")?.querySelector("[data-role='characters-tone-evidence']");
    expect(thin?.textContent).toContain("3 judged runs");
    expect(thin?.querySelector("[data-role='characters-tone-thin']")).not.toBeNull();
    expect(solid?.textContent).toContain("12 judged runs");
    expect(solid?.querySelector("[data-role='characters-tone-thin']")).toBeNull();
  });
});

describe("Make active previews before it switches", () => {
  const results = genToneResults("t-scale", {
    theatrical: [{ judged: 12, fail: 2 }],
    "gleeful-chaos": [{ judged: 2, fail: 1, redLine: 1 }, { judged: 4 }],
    "deadpan-bureaucrat": [{ judged: 8, fail: 1 }],
  });

  it("the first click sends nothing and shows both tones' records from the named tournament", async () => {
    const m = mount(["malakor", "package", "tones"], { results });
    await settle(6);
    await makeActive("Gleeful chaos");
    expect(m.fetch.calls).not.toContain("character.tone.set");
    const p = role("characters-tone-preview")?.textContent ?? "";
    expect(p).toContain("Theatrical");
    expect(p).toContain("Gleeful chaos");
    expect(p).toContain("t-scale");
    expect(p).toContain("12 judged runs");
    expect(p).toContain("2 of 24 rule checks failed");
    expect(p).toContain("no red-line fail");
    expect(p).toContain("6 judged runs");
    expect(p).toContain("2 of 12 rule checks failed");
    expect(p).toContain("1 red-line fail");
  });

  it("Confirm sends character.tone.set once and the hero switches; Cancel sends nothing", async () => {
    const tone = { value: "theatrical" };
    const toneSent: string[] = [];
    mount(["malakor", "package", "tones"], { results, tone, toneSent });
    await settle(6);
    await makeActive("Deadpan bureaucrat");
    await press(role("characters-tone-preview")?.querySelector("[data-role='characters-tone-cancel']"));
    expect(role("characters-tone-preview")).toBeNull();
    expect(toneSent).toEqual([]);
    expect(role("characters-tone-hero")?.textContent).toContain("Theatrical");
    await makeActive("Gleeful chaos");
    await press(role("characters-tone-preview")?.querySelector("[data-role='characters-tone-confirm']"));
    await settle();
    expect(toneSent).toHaveLength(1);
    expect(toneSent[0]).toContain('"params":{"id":"malakor","tone":"gleeful-chaos"}');
    expect(role("characters-tone-hero")?.textContent).toContain("Gleeful chaos");
  });

  it("warns with an alert and relabels Confirm when the candidate failed a red line", async () => {
    mount(["malakor", "package", "tones"], { results });
    await settle(6);
    await makeActive("Gleeful chaos");
    const p = role("characters-tone-preview");
    expect(p?.querySelector("[role='alert']")?.textContent).toContain("red line");
    expect(p?.querySelector("[data-role='characters-tone-confirm']")?.textContent).toBe("Make active anyway");
    await press(p?.querySelector("[data-role='characters-tone-cancel']"));
    await makeActive("Deadpan bureaucrat");
    expect(role("characters-tone-preview")?.querySelector("[role='alert']")).toBeNull();
    expect(role("characters-tone-preview")?.querySelector("[data-role='characters-tone-confirm']")?.textContent).toBe("Make active");
  });

  it("says no tournament has measured the tones when none ran, and Confirm still switches", async () => {
    const tone = { value: "theatrical" };
    const toneSent: string[] = [];
    mount(["malakor", "package", "tones"], { tournaments: [], tone, toneSent });
    await settle(6);
    await makeActive("Gleeful chaos");
    expect(role("characters-tone-preview")?.textContent).toContain("No tournament has measured these tones yet");
    await press(role("characters-tone-preview")?.querySelector("[data-role='characters-tone-confirm']"));
    await settle();
    expect(toneSent).toHaveLength(1);
    expect(role("characters-tone-hero")?.textContent).toContain("Gleeful chaos");
  });
});

describe("Tones and Situations read one evidence source", () => {
  it("picking another 'Verdicts from' tournament on Situations changes the tournament the Tones name", async () => {
    const m = mountPage(<Routed />, daemon({ results: genToneResults("t-scale", { theatrical: [{ judged: 12 }] }) }), "#/characters/malakor/package/situations");
    dispose = m.dispose;
    await settle(6);
    const select = role("characters-situation-results")?.querySelector("select") ?? null;
    expect(select?.value).toBe("t-scale");
    fireEvent.change(select ?? document.body, { target: { value: "t-pilot" } });
    await settle(6);
    act(() => m.router.navigate(pkgRoute("malakor", "tones")));
    await settle(6);
    const ev = role("characters-tone-hero")?.querySelector("[data-role='characters-tone-evidence']")?.textContent ?? "";
    expect(ev).toContain("t-pilot");
    expect(ev).not.toContain("t-scale");
  });
});
