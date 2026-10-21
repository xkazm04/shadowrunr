import { describe, expect, it } from "vitest";
import { composerState, draftOf } from "./planState";
import { genPlan } from "./testgen";

const live = (report_id: string, option_ids: string[]) => ({ ...genPlan(option_ids), report_id });

describe("composerState", () => {
  it("names no live plan as none", () => {
    expect(composerState(undefined, "rec-A", [])).toEqual({ kind: "none" });
  });

  it("names a live plan for this report with the same options as live-here, same", () => {
    expect(composerState(live("rec-A", ["opt-1", "opt-3"]), "rec-A", ["opt-3", "opt-1"])).toMatchObject({ kind: "live-here", same: true, added: [], removed: [] });
  });

  it("names what a draft drops and adds against the live plan", () => {
    expect(composerState(live("rec-A", ["opt-1", "opt-3"]), "rec-A", ["opt-1"])).toMatchObject({ kind: "live-here", same: false, removed: ["opt-3"], added: [] });
    expect(composerState(live("rec-A", ["opt-1"]), "rec-A", ["opt-1", "opt-4"])).toMatchObject({ kind: "live-here", same: false, removed: [], added: ["opt-4"] });
  });

  it("names a live plan sealed from another report as live-elsewhere", () => {
    expect(composerState(live("rec-B", ["opt-1"]), "rec-A", [])).toMatchObject({ kind: "live-elsewhere", from: "rec-B" });
  });

  it("names a failed read as unknown, carrying the reason", () => {
    expect(composerState({ failed: "The engine failed" }, "rec-A", [])).toEqual({ kind: "unknown", reason: "The engine failed" });
  });

  it("names a read still in flight as loading", () => {
    expect(composerState("loading", "rec-A", [])).toEqual({ kind: "loading" });
  });
});

describe("draftOf", () => {
  it("seeds an untouched draft from the live plan of this report", () => {
    expect(draftOf(live("rec-A", ["opt-1", "opt-3"]), "rec-A", undefined)).toEqual(["opt-1", "opt-3"]);
  });

  it("keeps a touched draft, even an empty one", () => {
    expect(draftOf(live("rec-A", ["opt-1", "opt-3"]), "rec-A", [])).toEqual([]);
    expect(draftOf(live("rec-A", ["opt-1", "opt-3"]), "rec-A", ["opt-2"])).toEqual(["opt-2"]);
  });

  it("never seeds from a plan of another report or from no plan", () => {
    expect(draftOf(live("rec-B", ["opt-1"]), "rec-A", undefined)).toEqual([]);
    expect(draftOf(undefined, "rec-A", undefined)).toEqual([]);
    expect(draftOf({ failed: "x" }, "rec-A", undefined)).toEqual([]);
  });
});
