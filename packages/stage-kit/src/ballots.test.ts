// The screen's ballot rule against the runtime's, on vectors the runtime generates
// (crates/shadowrunr-segments/tests/ballot_vectors.rs). Never hand-edit the vectors: a Rust rule
// change regenerates them and this test fails until the screen follows.

import { describe, expect, it } from "vitest";

import type { StreamEvent, VoteTally } from "@shadowrunr/contract";
import { BALLOT_DEVIATIONS, matchOption, observedTally } from "./ballots";
import raw from "./generated/ballot-vectors.json";
import { parseStreamEvent } from "./parse";
import { foldEvents } from "./scene";
import { leaders, pollBars } from "./tallies";

interface Scenario {
  name: string;
  what: string;
  events: StreamEvent[];
  poll_id: string;
  expected: VoteTally[];
}
interface Vectors {
  scenarios: Scenario[];
  deviations: { id: string; scenario: string; note: string }[];
}

/** The vectors, every event through the contract's door: a transcript the screen could not read
 * fails here rather than passing on a hand-shaped event. */
const vectors: Vectors = {
  deviations: raw.deviations,
  scenarios: raw.scenarios.map((s) => ({
    name: s.name,
    what: s.what,
    poll_id: s.poll_id,
    expected: s.expected,
    events: s.events.map((e: unknown, i: number) => {
      const r = parseStreamEvent(e);
      if (!r.ok) throw new Error(`${s.name} event ${i}: ${r.message}`);
      return r.value;
    }),
  })),
};
const byName = (name: string): Scenario => {
  const s = vectors.scenarios.find((x) => x.name === name);
  if (!s) throw new Error(`no scenario ${name}`);
  return s;
};

/** The screen's chat-observed tally of the scenario's poll, after folding its events. */
function screenTally(s: Scenario): VoteTally[] {
  const scene = foldEvents(s.events);
  expect(scene.activePoll).toBe(s.poll_id);
  return observedTally(scene.polls[s.poll_id]!);
}

function agrees(have: readonly VoteTally[], want: readonly VoteTally[]): boolean {
  return (
    have.length === want.length &&
    have.every((h, i) => h.option === want[i]!.option && h.voters === want[i]!.voters && Math.abs(h.weight - want[i]!.weight) <= 1e-9)
  );
}

function expectAgrees(have: readonly VoteTally[], want: readonly VoteTally[]): void {
  expect(have.map((t) => t.option)).toEqual(want.map((t) => t.option));
  expect(have.map((t) => t.voters)).toEqual(want.map((t) => t.voters));
  have.forEach((t, i) => expect(t.weight).toBeCloseTo(want[i]!.weight, 9));
}

const declared = new Set(vectors.deviations.map((d) => d.scenario));

describe("ballot vectors from the runtime", () => {
  it("cover prefix, index, case/trim, re-vote, under-rank, summon by poll kind, unknown option and a zero-voter row", () => {
    expect(vectors.scenarios.length).toBeGreaterThanOrEqual(8);
    const names = vectors.scenarios.map((s) => s.name);
    for (const n of ["prefix_is_not_a_ballot", "summon_in_crowd_poll_is_not_a_ballot", "summon_and_vote_in_foil_vote", "crowd_poll_ranks_case_index_revote", "trimmed_exact_text", "unknown_option_and_out_of_range_index", "under_rank_revote_keeps_the_ballot", "zero_voter_row"]) {
      expect(names).toContain(n);
    }
    expect(byName("zero_voter_row").expected.some((t) => t.voters === 0)).toBe(true);
  });

  for (const s of vectors.scenarios.filter((x) => !declared.has(x.name))) {
    it(`agrees with the runtime: ${s.name}`, () => {
      expectAgrees(screenTally(s), s.expected);
    });
  }

  it("declares exactly the screen-side gaps the stage-kit knows, and each still diverges", () => {
    expect(vectors.deviations.map((d) => d.id)).toEqual([...BALLOT_DEVIATIONS]);
    for (const d of vectors.deviations) {
      // A declared gap that closed must be retired from the list, not left as cover.
      expect(agrees(screenTally(byName(d.scenario)), byName(d.scenario).expected), d.id).toBe(false);
    }
  });
});

describe("the acceptance cases, by name", () => {
  it("a unique prefix is not a ballot, on screen as in the runtime", () => {
    const s = byName("prefix_is_not_a_ballot");
    const disruptor = (t: VoteTally[]) => t.find((x) => x.option === "the-disruptor")!.voters;
    expect(disruptor(s.expected)).toBe(0);
    expect(disruptor(screenTally(s))).toBe(0);
  });

  it("`!summon` counts nothing in an op-opened crowd poll", () => {
    const s = byName("summon_in_crowd_poll_is_not_a_ballot");
    expect(s.expected.every((t) => t.voters === 0)).toBe(true);
    expect(screenTally(s).every((t) => t.voters === 0 && t.weight === 0)).toBe(true);
  });

  it("ranks, case, index and a re-vote fold to the runtime's last tally", () => {
    const s = byName("crowd_poll_ranks_case_index_revote");
    expect(s.events.filter((e) => e.type === "vote").length).toBeGreaterThan(0);
    expectAgrees(screenTally(s), s.expected);
  });

  it("before the runtime's first tally the bars cannot crown an option the runtime did not count", () => {
    const s = byName("prefix_is_not_a_ballot");
    const scene = foldEvents(s.events.filter((e) => e.type !== "vote"));
    const bars = pollBars(scene.polls[s.poll_id]!);
    expect(bars.filter((b) => b.leading).map((b) => b.option)).toEqual(["the-life-coach"]);
  });
});

describe("matchOption", () => {
  it("matches by index first, then trimmed exact text, ASCII case folded, never by prefix", () => {
    const o = ["the-disruptor", "the-inspector", "HEX-04"];
    expect(matchOption(o, "HEX-04")).toBe("HEX-04");
    expect(matchOption(o, "hex-04")).toBe("HEX-04");
    expect(matchOption(o, "  hex-04 ")).toBe("HEX-04");
    expect(matchOption(o, "2")).toBe("the-inspector");
    expect(matchOption(o, "+2")).toBe("the-inspector");
    expect(matchOption(o, "9")).toBeNull();
    expect(matchOption(o, "0")).toBeNull();
    expect(matchOption(o, "the-d")).toBeNull();
    expect(matchOption(o, "the")).toBeNull();
    expect(matchOption(o, " ")).toBeNull();
    expect(matchOption(["2", "1"], "1")).toBe("2");
    expect(matchOption(["10", "20"], "20")).toBe("20");
    expect(matchOption(["Über"], "über")).toBeNull();
  });
});

describe("leaders", () => {
  it("skips an option nobody voted for, whatever its weight, as the runtime's outcome() does", () => {
    expect(leaders([{ option: "A", weight: 1.4, voters: 0 }, { option: "B", weight: 1, voters: 1 }])).toEqual(["B"]);
    expect(leaders([{ option: "A", weight: 0, voters: 0 }])).toEqual([]);
  });
});
