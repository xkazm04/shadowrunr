// The Moderation tab's pure half: the rule form and its round trip to a ModRule, log-entry
// reading, labels and the arming summary. No React here, so the tests read it directly.
import type { ModActionKind, ModDecision, ModLogEntry, ModRule, ModRuleKind, ModUnban, PlatformState, PlatformStatus } from "@shadowrunr/contract";

export const KIND_ORDER: readonly ModRuleKind[] = ["pattern", "plain"];
export const ACTION_ORDER: readonly ModActionKind[] = ["delete", "timeout", "ban"];
export const KIND_NAME: Readonly<Record<ModRuleKind, string>> = { pattern: "Pattern", plain: "Plain language" };
export const ACTION_NAME: Readonly<Record<ModActionKind, string>> = { delete: "Delete", timeout: "Timeout", ban: "Ban" };
export const STATE_NAME: Readonly<Record<PlatformState, string>> = { connected: "Connected", connecting: "Connecting", disconnected: "Disconnected", error: "Error" };
export const STATE_TONE: Readonly<Record<PlatformState, "ok" | "caution" | "muted" | "fail">> = { connected: "ok", connecting: "caution", disconnected: "muted", error: "fail" };

/** The one facet group the rules table offers: every rule, a kind, or an action. */
export type RuleFacet = "all" | ModRuleKind | ModActionKind;
export const RULE_FACETS: readonly { id: RuleFacet; label: string }[] = [
  { id: "all", label: "All" },
  ...KIND_ORDER.map((id) => ({ id, label: KIND_NAME[id] })),
  ...ACTION_ORDER.map((id) => ({ id, label: ACTION_NAME[id] })),
];
export const ruleInFacet = (r: ModRule, f: RuleFacet): boolean => f === "all" || r.kind === f || r.action === f;

export const plural = (n: number, one: string, many = `${one}s`): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
const minutes = (s: number): string => (s % 60 === 0 ? plural(s / 60, "min", "min") : plural(s, "s", "s"));

/** "Timeout 10 min", "Ban". */
export const actionText = (action: ModActionKind, timeoutS: number | undefined): string =>
  action === "timeout" && timeoutS !== undefined ? `${ACTION_NAME.timeout} ${minutes(timeoutS)}` : ACTION_NAME[action];

/** "3 in 10 min, then Ban", or a dash. */
export const strikesText = (r: ModRule): string =>
  r.strikes ? `${r.strikes.count} in ${minutes(r.strikes.window_s)}, then ${actionText(r.strikes.then, r.strikes.then_timeout_s)}` : "—";

/** The rule's matcher as the table prints it. */
export const matcherText = (r: ModRule): string => (r.kind === "pattern" ? (r.pattern ?? "") : (r.text ?? ""));

/** `[a-z0-9-]` from a name: "No links, please" -> "no-links-please". */
export const slugOf = (name: string): string =>
  name
    .toLowerCase()
    .normalize("NFKD")
    .replace(/\p{M}+/gu, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);

/** The drawer's draft: every number as typed, so a half-typed field stays on screen. */
export interface RuleForm {
  id: string;
  /** The id was typed by hand: the name no longer derives it. */
  idTouched: boolean;
  name: string;
  kind: ModRuleKind;
  matcher: string;
  action: ModActionKind;
  timeoutMin: string;
  strikes: boolean;
  count: string;
  windowMin: string;
  then: ModActionKind;
  thenTimeoutMin: string;
  enabled: boolean;
}

export const blankForm = (): RuleForm => ({
  id: "",
  idTouched: false,
  name: "",
  kind: "pattern",
  matcher: "",
  action: "delete",
  timeoutMin: "10",
  strikes: false,
  count: "3",
  windowMin: "10",
  then: "timeout",
  thenTimeoutMin: "10",
  enabled: true,
});

const minText = (s: number | undefined, dflt: string): string => (s === undefined ? dflt : String(Math.round((s / 60) * 100) / 100));

export function formOf(r: ModRule): RuleForm {
  const b = blankForm();
  return {
    ...b,
    id: r.id,
    idTouched: true,
    name: r.name,
    kind: r.kind,
    matcher: matcherText(r),
    action: r.action,
    timeoutMin: minText(r.timeout_s, b.timeoutMin),
    enabled: r.enabled,
    ...(r.strikes
      ? { strikes: true, count: String(r.strikes.count), windowMin: minText(r.strikes.window_s, b.windowMin), then: r.strikes.then, thenTimeoutMin: minText(r.strikes.then_timeout_s, b.thenTimeoutMin) }
      : {}),
  };
}

/** Applies a name edit: on add the id follows the name until it is typed by hand. */
export const withName = (f: RuleForm, name: string): RuleForm => ({ ...f, name, id: f.idTouched ? f.id : slugOf(name) });

const positive = (s: string): number | undefined => {
  const t = s.trim();
  if (!/^\d+(\.\d+)?$/.test(t)) return undefined;
  const n = Number(t);
  return n > 0 ? n : undefined;
};
const secondsOf = (min: string): number | undefined => {
  const n = positive(min);
  return n === undefined ? undefined : Math.round(n * 60);
};

/**
 * The form as a ModRule, or the first thing wrong with it. `taken` are the other rules' ids. The
 * daemon validates again (a pattern that does not compile); this catches what the form can.
 * Optional fields are omitted, never null.
 */
export function ruleOf(f: RuleForm, taken: readonly string[]): { ok: true; rule: ModRule } | { ok: false; error: string } {
  const name = f.name.trim();
  const id = f.id.trim();
  const matcher = f.matcher.trim();
  if (name === "") return { ok: false, error: "Name the rule." };
  if (!/^[a-z0-9-]+$/.test(id)) return { ok: false, error: "The id is lowercase letters, digits and dashes." };
  if (taken.includes(id)) return { ok: false, error: `Another rule already has the id "${id}".` };
  if (matcher === "") return { ok: false, error: f.kind === "pattern" ? "Write the pattern." : "Describe what the rule catches." };
  const rule: ModRule = { id, name, kind: f.kind, action: f.action, enabled: f.enabled, ...(f.kind === "pattern" ? { pattern: matcher } : { text: matcher }) };
  if (f.action === "timeout") {
    const s = secondsOf(f.timeoutMin);
    if (s === undefined) return { ok: false, error: "The timeout is a number of minutes above 0." };
    rule.timeout_s = s;
  }
  if (f.strikes) {
    const count = positive(f.count);
    const window = secondsOf(f.windowMin);
    if (count === undefined || !Number.isInteger(count) || count < 2) return { ok: false, error: "Strikes count is a whole number, 2 or more." };
    if (window === undefined) return { ok: false, error: "The strikes window is a number of minutes above 0." };
    rule.strikes = { count, window_s: window, then: f.then };
    if (f.then === "timeout") {
      const s = secondsOf(f.thenTimeoutMin);
      if (s === undefined) return { ok: false, error: "The strikes timeout is a number of minutes above 0." };
      rule.strikes.then_timeout_s = s;
    }
  }
  return { ok: true, rule };
}

/** The list with `rule` in place of the one with id `replacing` (or appended on add). */
export function upsert(rules: readonly ModRule[], rule: ModRule, replacing: string | undefined): ModRule[] {
  if (replacing === undefined) return [...rules, rule];
  return rules.map((r) => (r.id === replacing ? rule : r));
}

export type LogRow = { kind: "decision"; d: ModDecision } | { kind: "unban"; u: ModUnban };

export const rowOf = (e: ModLogEntry): LogRow => (e.kind === "unban" ? { kind: "unban", u: e.unban } : { kind: "decision", d: e.decision });

/** Executed bans and timeouts can be lifted; nothing else can. */
export const canUnban = (d: ModDecision): boolean => d.executed && (d.action === "ban" || d.action === "timeout");

/** The rule's name, else its id (a rule deleted since). */
export const ruleName = (rules: readonly ModRule[] | undefined, id: string): string => rules?.find((r) => r.id === id)?.name ?? id;

/** The arming question: the shadow counts and the bans among the last log entries. */
export function armingText(decisions: number, entries: readonly ModLogEntry[] | undefined): string {
  const bans = entries === undefined ? undefined : entries.map(rowOf).filter((r) => r.kind === "decision" && r.d.action === "ban").length;
  const banText = bans === undefined ? "bans unknown" : plural(bans, "ban");
  return `In shadow: ${plural(decisions, "would-be action")}, ${banText}. Arm the moderator?`;
}

/** Twitch as the moderator sees it; absent means it was never configured. */
export const twitchOf = (platforms: readonly PlatformStatus[]): PlatformStatus =>
  platforms.find((p) => p.platform === "twitch") ?? { platform: "twitch", state: "disconnected", detail: "Twitch is not configured in shadowrunr.toml." };

/** "4:59" from seconds left. */
export const countdown = (s: number): string => `${Math.floor(Math.max(0, s) / 60)}:${String(Math.max(0, s) % 60).padStart(2, "0")}`;
