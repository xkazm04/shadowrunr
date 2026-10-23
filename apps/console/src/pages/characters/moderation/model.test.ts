// The Moderation tab's pure half: the rule form round trip, its refusals, log rows and labels.
import { describe, expect, it } from "vitest";
import type { ModRule } from "@shadowrunr/contract";
import { armingText, blankForm, canUnban, countdown, formOf, rowOf, ruleOf, slugOf, strikesText, withName } from "./model";

const SPAM: ModRule = { id: "spam", name: "Spam", kind: "plain", text: "copy-paste waves", action: "timeout", timeout_s: 90, enabled: false, strikes: { count: 3, window_s: 600, then: "timeout", then_timeout_s: 3600 } };

describe("moderation model", () => {
  it("round-trips a rule through the form, omitting what is unset", () => {
    expect(ruleOf(formOf(SPAM), [])).toEqual({ ok: true, rule: SPAM });
    const f = { ...withName(blankForm(), "No links!"), matcher: "https?://" };
    expect(f.id).toBe("no-links");
    const r = ruleOf(f, []);
    expect(r).toEqual({ ok: true, rule: { id: "no-links", name: "No links!", kind: "pattern", pattern: "https?://", action: "delete", enabled: true } });
    expect(withName({ ...f, idTouched: true, id: "mine" }, "Other").id).toBe("mine");
  });

  it("refuses what the form can catch", () => {
    const base = { ...withName(blankForm(), "Caps"), matcher: "[A-Z]+" };
    const err = (f: typeof base, taken: string[] = []) => {
      const r = ruleOf(f, taken);
      return r.ok ? "" : r.error;
    };
    expect(err({ ...base, name: " " })).toContain("Name");
    expect(err({ ...base, id: "Caps!" })).toContain("lowercase");
    expect(err(base, ["caps"])).toContain("already");
    expect(err({ ...base, matcher: "" })).toContain("pattern");
    expect(err({ ...base, action: "timeout", timeoutMin: "0" })).toContain("timeout");
    expect(err({ ...base, strikes: true, count: "1" })).toContain("Strikes");
    expect(err({ ...base, strikes: true, windowMin: "x" })).toContain("window");
  });

  it("reads log rows by their kind tag, and only executed bans and timeouts unban", () => {
    const d = { id: "d", ts_ms: 0, platform: "twitch", viewer_id: "v", viewer_name: "n", message_id: "m", rule_id: "r", action: "ban", mode: "armed", executed: true } as const;
    const u = { ts_ms: 0, platform: "twitch", viewer_id: "v", executed: true } as const;
    expect(rowOf({ kind: "decision", decision: d })).toEqual({ kind: "decision", d });
    expect(rowOf({ kind: "unban", unban: u })).toEqual({ kind: "unban", u });
    expect(canUnban(d)).toBe(true);
    expect(canUnban({ ...d, executed: false })).toBe(false);
    expect(canUnban({ ...d, action: "delete" })).toBe(false);
    expect(armingText(1, [{ kind: "decision", decision: d }, { kind: "unban", unban: u }])).toBe("In shadow: 1 would-be action, 1 ban. Arm the moderator?");
    expect(armingText(0, undefined)).toBe("In shadow: 0 would-be actions, bans unknown. Arm the moderator?");
  });

  it("labels", () => {
    expect(slugOf("  Ünïcode -- Spam  ")).toBe("unicode-spam");
    expect(strikesText(SPAM)).toBe("3 in 10 min, then Timeout 60 min");
    expect(countdown(299)).toBe("4:59");
    expect(countdown(-3)).toBe("0:00");
  });
});
