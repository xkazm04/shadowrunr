// The Moderation tab in jsdom against a scripted daemon that keeps the rules, the mode and the
// log in memory: when the tab shows, arming with its shadow counts, rule add/edit/toggle/delete
// as whole-list writes with an inline refusal, the test box, the log with unban, and the Twitch
// device-code login. Every fixture is fictional.
import { act, cleanup, fireEvent } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ModDecision, ModeratorStatus, ModRule, ModRulesTestResult, PlatformStatus } from "@shadowrunr/contract";
import { fail, mountPage, ok, role, roles, settle, type Script } from "../../../kit/testing";
import { Layer2 } from "../Layer2";
import { malakorDetail } from "../testgen";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
});

const LINKS: ModRule = { id: "no-links", name: "No links", kind: "pattern", pattern: "https?://", action: "delete", enabled: true };
const SPAM: ModRule = { id: "spam", name: "Spam waves", kind: "plain", text: "copy-paste spam", action: "timeout", timeout_s: 600, enabled: true, strikes: { count: 3, window_s: 600, then: "ban" } };

const decision = (o: Partial<ModDecision>): ModDecision & { kind: "decision" } => ({
  kind: "decision",
  id: "d1",
  ts_ms: Date.UTC(2026, 9, 23, 20, 15),
  platform: "twitch",
  viewer_id: "v1",
  viewer_name: "fictional_viewer",
  message_id: "m1",
  rule_id: "no-links",
  action: "delete",
  mode: "shadow",
  executed: false,
  ...o,
});

interface Daemon {
  status: ModeratorStatus | "error";
  rules: ModRule[];
  log: unknown[];
  sent: { method: string; params: string }[];
  /** Each moderator.rules.set's list as the console handed it to the layer. */
  sets: ModRule[][];
  refuseRules?: string;
  test?: ModRulesTestResult;
}

function daemon(o: Partial<Daemon> = {}): Daemon & { script: Script } {
  const twitch: PlatformStatus = { platform: "twitch", state: "disconnected", detail: "Not logged in yet." };
  const d: Daemon = {
    status: { mode: "shadow", character: "malakor", platforms: [twitch], counts: { decisions: 12, executed: 0 } },
    rules: [LINKS, SPAM],
    log: [
      decision({ id: "d3", action: "ban", mode: "armed", executed: true, viewer_id: "v3", viewer_name: "fictional_banned", line: "Begone, mortal." }),
      decision({ id: "d2", action: "timeout", timeout_s: 600, mode: "armed", executed: false, error: "rate limited", rule_id: "gone-rule" }),
      { kind: "unban", ts_ms: Date.UTC(2026, 9, 23, 20, 10), platform: "twitch", viewer_id: "v3", executed: true },
      decision({ id: "d1", action: "ban" }),
    ],
    sent: [],
    sets: [],
    ...o,
  };
  const script: Script = (method, raw) => {
    const params = raw.slice(raw.indexOf('"params":') + 9, -1);
    d.sent.push({ method, params });
    const st = () => (d.status === "error" ? fail("internal", "moderator not built") : ok(d.status));
    switch (method) {
      case "character.get":
        return ok(malakorDetail());
      case "moderator.status":
        return st();
      case "moderator.rules.list":
        return ok({ rules: d.rules });
      case "moderator.rules.set":
        if (d.refuseRules) return fail("invalid_params", d.refuseRules);
        d.rules = d.sets.at(-1) ?? d.rules;
        return ok({ rules: d.rules });
      case "moderator.mode.set":
        if (d.status !== "error") d.status = { ...d.status, mode: raw.includes('"armed"') ? "armed" : "shadow" };
        return st();
      case "moderator.log":
        return ok({ entries: d.log });
      case "moderator.rules.test":
        return ok(d.test ?? { decisions: [], disclosure: false });
      case "moderator.unban": {
        const u = { kind: "unban", ts_ms: Date.UTC(2026, 9, 23, 21), platform: "twitch", viewer_id: "v3", executed: true };
        d.log = [u, ...d.log];
        return ok(u);
      }
      case "twitch.connect":
        if (d.status !== "error") d.status = { ...d.status, platforms: [{ platform: "twitch", state: "connecting" }] };
        return ok({ verification_uri: "https://www.twitch.tv/activate", user_code: "WXYZ-1234", expires_s: 300 });
      case "twitch.disconnect":
        if (d.status !== "error") d.status = { ...d.status, platforms: [twitch] };
        return st();
      default:
        return fail("method_not_found", method);
    }
  };
  return Object.assign(d, { script });
}

async function mount(d: Daemon & { script: Script }, tab = "moderation") {
  const sub = ["malakor", "package", tab];
  const m = mountPage(<Layer2 sub={sub} />, d.script, `#/characters/${sub.join("/")}`);
  dispose = m.dispose;
  // The written list, read before it is serialised (tests do not parse JSON).
  const mutate = m.layer.mutate.bind(m.layer);
  m.layer.mutate = (method, params, opts) => {
    if (method === "moderator.rules.set") d.sets.push((params as { rules: ModRule[] }).rules);
    return mutate(method, params, opts);
  };
  await settle(10);
  // The tab is a lazy chunk: its first import can outlast the settle rounds.
  for (let i = 0; i < 50 && tab === "moderation" && role("characters-moderation-tab") === null && role("characters-package")?.textContent?.includes("not the moderator") !== true; i++) {
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
  }
  await settle(6);
  return m;
}

const click = (el: Element | null | undefined) => act(async () => fireEvent.click(el ?? document.body));
const type = (r: string, value: string) => act(async () => fireEvent.change(role(r) ?? document.body, { target: { value } }));
const tabNames = () => [...document.querySelectorAll("[role='tab']")].map((t) => t.textContent);
const sentOf = (d: Daemon, method: string) => d.sent.filter((s) => s.method === method).map((s) => s.params);
const rulesSent = (d: Daemon) => d.sets;
const ruleRow = (name: string) => roles("moderation-rule-name").find((n) => n.textContent === name)?.closest("tr");

describe("characters/<id>/package/moderation", () => {
  it("shows the tab only on the moderator's package, and hides it when moderator.status errors", async () => {
    await mount(daemon(), "constitution");
    expect(tabNames()).toContain("Moderation");
    cleanup();
    dispose?.();
    const other = daemon();
    if (other.status !== "error") other.status = { ...other.status, character: "warden" };
    await mount(other, "constitution");
    expect(tabNames()).not.toContain("Moderation");
    cleanup();
    dispose?.();
    await mount(daemon({ status: "error" }), "constitution");
    expect(tabNames()).not.toContain("Moderation");
    expect(tabNames()).toContain("Tone Lab");
    cleanup();
    dispose?.();
    await mount(daemon({ status: "error" }));
    expect(role("characters-moderation-tab")).toBeNull();
    expect(document.body.textContent).toContain("Arch-Lich Malakor is not the moderator");
  });

  it("asks before arming with the shadow counts, and disarms without asking", async () => {
    const d = daemon();
    await mount(d);
    expect(role("moderation-mode")?.getAttribute("data-mode")).toBe("shadow");
    await click(role("moderation-mode")?.querySelector("[role='switch']"));
    expect(sentOf(d, "moderator.mode.set")).toEqual([]);
    expect(role("moderation-arm")?.textContent).toContain("In shadow: 12 would-be actions, 2 bans. Arm the moderator?");
    await click(role("moderation-arm-yes"));
    await settle();
    expect(sentOf(d, "moderator.mode.set")).toEqual([JSON.stringify({ mode: "armed" })]);
    expect(role("moderation-mode")?.getAttribute("data-mode")).toBe("armed");
    expect(role("moderation-arm")).toBeNull();
    await click(role("moderation-mode")?.querySelector("[role='switch']"));
    await settle();
    expect(role("moderation-arm")).toBeNull();
    expect(sentOf(d, "moderator.mode.set")).toEqual([JSON.stringify({ mode: "armed" }), JSON.stringify({ mode: "shadow" })]);
    expect(role("moderation-mode")?.getAttribute("data-mode")).toBe("shadow");
  });

  it("sends the whole list on add, edit, toggle and delete, and prints a refusal inline", async () => {
    const d = daemon();
    await mount(d);
    expect(roles("moderation-rule-name").map((n) => n.textContent)).toEqual(["No links", "Spam waves"]);
    expect(ruleRow("Spam waves")?.textContent).toContain("Timeout 10 min");
    expect(ruleRow("Spam waves")?.textContent).toContain("3 in 10 min, then Ban");

    // Add: the id follows the name.
    await click(role("moderation-rule-add"));
    await type("moderation-rule-name-input", "Caps Lock Rage");
    expect((role("moderation-rule-id") as HTMLInputElement).value).toBe("caps-lock-rage");
    await type("moderation-rule-matcher", "[A-Z]{20,}");
    await click(role("moderation-rule-save"));
    await settle();
    const caps: ModRule = { id: "caps-lock-rage", name: "Caps Lock Rage", kind: "pattern", action: "delete", enabled: true, pattern: "[A-Z]{20,}" };
    expect(rulesSent(d)[0]).toEqual([LINKS, SPAM, caps]);
    expect(role("moderation-rule-drawer")?.getAttribute("data-open")).toBe("false");

    // Edit: a timeout of 5 minutes, in place.
    await click(ruleRow("No links")?.querySelector("[data-role='moderation-rule-edit']"));
    await click([...(role("moderation-rule-form")?.querySelectorAll("button[aria-pressed]") ?? [])].find((b) => b.textContent === "Timeout"));
    await type("moderation-rule-timeout", "5");
    await click(role("moderation-rule-save"));
    await settle();
    expect(rulesSent(d)[1]?.[0]).toEqual({ ...LINKS, action: "timeout", timeout_s: 300 });
    expect(rulesSent(d)[1]).toHaveLength(3);

    // Toggle.
    await click(ruleRow("Spam waves")?.querySelector("[role='switch']"));
    await settle();
    expect(rulesSent(d)[2]?.[1]).toEqual({ ...SPAM, enabled: false });

    // Delete asks first.
    await click(ruleRow("Caps Lock Rage")?.querySelector("[data-role='moderation-rule-delete']"));
    expect(rulesSent(d)).toHaveLength(3);
    expect(role("moderation-rule-delete-confirm")?.textContent).toContain('Delete "Caps Lock Rage"?');
    await click(role("moderation-rule-delete-confirm-yes"));
    await settle();
    expect(rulesSent(d)[3]?.map((r) => r.id)).toEqual(["no-links", "spam"]);

    // A refusal prints inline, in the drawer and in the panel.
    d.refuseRules = "rule no-links: pattern does not compile";
    await click(role("moderation-rule-add"));
    await type("moderation-rule-name-input", "Broken");
    await type("moderation-rule-matcher", "(");
    await click(role("moderation-rule-save"));
    await settle();
    expect(role("moderation-rule-error")?.textContent).toContain("pattern does not compile");
    expect(role("moderation-rule-drawer")?.getAttribute("data-open")).toBe("true");
    await click(role("drawer-close"));
    await click(ruleRow("No links")?.querySelector("[role='switch']"));
    await settle();
    expect(role("moderation-rules-error")?.textContent).toContain("pattern does not compile");
  });

  it("runs the test box: a match, no match and a self-disclosure", async () => {
    const d = daemon({ test: { decisions: [decision({ rule_id: "spam", action: "timeout", timeout_s: 600 })], disclosure: false } });
    await mount(d);
    expect((role("moderation-test-run") as HTMLButtonElement).disabled).toBe(true);
    await type("moderation-test-text", "buy followers now");
    await type("moderation-test-viewer", "fictional_spammer");
    await click(role("moderation-test-run"));
    await settle();
    expect(sentOf(d, "moderator.rules.test")).toEqual([JSON.stringify({ text: "buy followers now", viewer_name: "fictional_spammer" })]);
    expect(role("moderation-test-match")?.textContent).toContain("Spam waves");
    expect(role("moderation-test-match")?.textContent).toContain("Timeout 10 min");
    d.test = { decisions: [], disclosure: false };
    await click(role("moderation-test-run"));
    await settle();
    expect(role("moderation-test-none")?.textContent).toBe("No rule matched");
    expect(role("moderation-test-match")).toBeNull();
    d.test = { decisions: [], disclosure: true };
    await click(role("moderation-test-run"));
    await settle();
    expect(role("moderation-test-disclosure")?.textContent).toContain("no rule acts on it");
    expect(role("moderation-test-none")).toBeNull();
  });

  it("renders decision and unban rows; Unban only on executed bans and timeouts, after a confirmation", async () => {
    const d = daemon();
    await mount(d);
    expect(sentOf(d, "moderator.log")[0]).toBe(JSON.stringify({ limit: 100 }));
    const rows = roles("moderation-log-decision");
    expect(rows).toHaveLength(3);
    expect(rows[0]?.textContent).toContain("fictional_banned");
    expect(rows[0]?.textContent).toContain("No links");
    expect(rows[0]?.textContent).toContain("Executed");
    expect(rows[0]?.textContent).toContain("Begone, mortal.");
    expect(rows[1]?.textContent).toContain("gone-rule");
    expect(rows[1]?.textContent).toContain("Failed");
    expect(rows[1]?.textContent).toContain("rate limited");
    expect(rows[2]?.textContent).toContain("Shadow");
    expect(rows.map((r) => r.querySelector("[data-role='moderation-unban']") !== null)).toEqual([true, false, false]);
    expect(role("moderation-log-unban")?.textContent).toContain("fictional_banned");
    await click(rows[0]?.querySelector("[data-role='moderation-unban']"));
    expect(sentOf(d, "moderator.unban")).toEqual([]);
    expect(role("moderation-unban-confirm")?.textContent).toContain("Lift the ban on fictional_banned?");
    await click(role("moderation-unban-confirm-yes"));
    await settle();
    expect(sentOf(d, "moderator.unban")).toEqual([JSON.stringify({ platform: "twitch", viewer_id: "v3" })]);
    expect(roles("moderation-log-unban")).toHaveLength(2);
  });

  it("shows the empty log, the Twitch login code, and asks before disconnecting", async () => {
    const d = daemon({ log: [] });
    await mount(d);
    expect(role("moderation-log")?.textContent).toContain("Nothing logged yet");
    expect(role("moderation-twitch-state")?.getAttribute("data-state")).toBe("disconnected");
    expect(role("moderation-twitch-detail")?.textContent).toBe("Not logged in yet.");
    await click(role("moderation-connect"));
    await settle();
    expect(role("moderation-user-code")?.textContent).toBe("WXYZ-1234");
    expect(role("moderation-verification-uri")?.getAttribute("href")).toBe("https://www.twitch.tv/activate");
    expect(role("moderation-code-expiry")?.textContent).toMatch(/Expires in [45]:\d\d/);
    expect(role("moderation-twitch-state")?.getAttribute("data-state")).toBe("connecting");
    await click(role("moderation-disconnect"));
    expect(sentOf(d, "twitch.disconnect")).toEqual([]);
    await click(role("moderation-disconnect-confirm-yes"));
    await settle();
    expect(sentOf(d, "twitch.disconnect")).toHaveLength(1);
    expect(role("moderation-twitch-state")?.getAttribute("data-state")).toBe("disconnected");
    expect(role("moderation-device-code")).toBeNull();
  });
});
