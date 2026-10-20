// Layer 2 of Session review: one sandbox session and its tabs, as routes:
//   review/<session>[/timeline[/<line>]]   review/<session>/types   review/<session>/player/<kind>
//   review/<session>/judge   review/<session>/moderation   review/<session>/calls
// The tabs come first; under them each tab has the page head, the verdict and its gate checks,
// the session's figures, then the tab's sections. That body is keyed on the tab and marked
// `data-rise`, so the shell's entrance rise staggers head, verdict and body on every tab change.
import type { ReviewSession } from "@shadowrunr/contract";
import { QueryView } from "../../kit/QueryView";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { buttonClass } from "../../ui/Button";
import { Chip, ChipRow, toneOfState } from "../../ui/Chip";
import { Head, Tabs, type TabItem } from "../../ui/Section";
import { StatBand } from "../../ui/StatBand";
import { StatePanel } from "../../ui/States";
import { EngineCalls } from "./EngineCalls";
import { useDerived, useFeedback, useSession } from "./hooks";
import { Judge } from "./Judge";
import styles from "./Layer2.module.css";
import { Moderation } from "./Moderation";
import { CHECK_NAME, CHECK_STATE_NAME, fmtClock, human, KIND_NAME, sessionTitle, SILENCE_LIMIT_S, type ShownKind } from "./names";
import { Player } from "./Player";
import { ReviewByType } from "./ReviewByType";
import type { TabId, TabProps } from "./tab";
import { Timeline } from "./Timeline";

const TAB_IDS: readonly TabId[] = ["timeline", "types", "player", "judge", "moderation", "calls"];
const isTab = (s: string | undefined): s is TabId => TAB_IDS.includes(s as TabId);
const isKind = (s: string | undefined): s is ShownKind => s !== undefined && s in KIND_NAME;

/** Each tab's lede under the head: what the tab is for, in a line. */
const LEDE: Readonly<Record<TabId, string>> = {
  timeline: "Every line he spoke, row by row. A row opens its lines; a line opens the moment, with the chat it answered and your marks.",
  types: "Pick a type and step through every line of it until each one carries your mark.",
  player: "One line at a time, with what came before it and the chat it answered, until each has your mark.",
  judge: "The rules the judge failed and the lines it points at, how the sampled lines scored, and what it wrote.",
  moderation: "Everything the gates changed, held back or refused, grouped by what happened.",
  calls: "Every engine call: by role first, then by minute, then the calls of one minute.",
};

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const [sessionId, head, arg, ...rest] = sub;
  const tab: TabId = isTab(head) ? head : "timeline";
  const known = sessionId !== undefined && (head === undefined || isTab(head)) && rest.length === 0 && (tab !== "player" || isKind(arg));
  const query = useSession(known ? sessionId : undefined);
  return (
    <div className={styles.root} data-page="review" data-role="review-layer2">
      {!known ? (
        <div data-rise>
          <StatePanel state="invalid" title={`There is no page at ${sub.join(" / ")}`} hint="Pick a session on the wall; each has a timeline, a review by type, the judge, moderation and engine calls." />
          <RouteLink to={pageRoute("review")} className={buttonClass("secondary")}>
            Back to all sessions
          </RouteLink>
        </div>
      ) : (
        <QueryView query={query}>{(rs) => <Session rs={rs} tab={tab} arg={arg} />}</QueryView>
      )}
    </div>
  );
}

function Session({ rs, tab, arg }: { rs: ReviewSession; tab: TabId; arg: string | undefined }) {
  const router = useRouter();
  const d = useDerived(rs);
  const feedback = useFeedback(rs);
  const id = rs.row.session_id;
  const kind = tab === "player" && isKind(arg) ? arg : undefined;
  const items: TabItem<TabId>[] = [
    { id: "timeline", label: "Timeline" },
    { id: "types", label: "Review by type" },
    ...(kind ? [{ id: "player" as const, label: `Reviewing: ${KIND_NAME[kind]}` }] : []),
    { id: "judge", label: rs.row.rule_fails ? `Judge · ${rs.row.rule_fails} fails` : "Judge" },
    { id: "moderation", label: "Moderation" },
    { id: "calls", label: "Engine calls" },
  ];
  const props: TabProps = {
    rs,
    d,
    feedback,
    openLine: (line) => router.navigate(pageRoute("review", id, "timeline", String(line))),
    openPlayer: (k) => router.navigate(pageRoute("review", id, "player", k)),
    ...(kind ? { kind } : {}),
    ...(tab === "timeline" && arg !== undefined && Number.isInteger(Number(arg)) ? { line: Number(arg) } : {}),
  };
  const tabName = items.find((t) => t.id === tab)?.label ?? "Timeline";
  const marked = d.says.filter((s) => feedback.byLine.has(s.line)).length;
  const fails = rs.row.rule_fails;
  return (
    <>
      <Tabs items={items} value={tab} label="Session review" onSelect={(t) => router.navigate(pageRoute("review", id, ...(t === "player" && kind ? ["player", kind] : [t])))} />
      <div key={tab} data-rise>
        <Head kicker={`Session review · ${tabName}`} title={sessionTitle(rs.row.minutes, rs.row.chaos)} lede={LEDE[tab]} />
        <div className={styles.verdict} data-role="review-verdict">
          <span className={rs.row.pass ? styles.pass : styles.fail}>{rs.row.pass ? "Passed" : "Failed"}</span>
          <ChipRow>
            {rs.row.checks.map((c) => (
              <Chip key={c.id} tone={toneOfState(c.state === "unmeasured" ? "not_evaluable" : c.state)} title={c.detail}>
                {CHECK_NAME[c.id] ?? human(c.id)} · {CHECK_STATE_NAME[c.state]}
              </Chip>
            ))}
          </ChipRow>
          <code className={styles.id}>{id}</code>
        </div>
        <div>
          <StatBand
            stats={[
              { label: "Of stream", value: fmtClock(d.lengthS) },
              { label: "His lines", value: d.says.length.toLocaleString("en-US") },
              { label: "Rule fails", value: fails ?? "–", ...(fails ? { tone: "fail" as const } : {}) },
              { label: "You marked", value: marked.toLocaleString("en-US") },
              { label: "Longest silence", value: `${rs.row.max_say_gap_s.toFixed(1)} s`, ...(rs.row.max_say_gap_s > SILENCE_LIMIT_S ? { tone: "fail" as const } : {}) },
            ]}
          />
          {!rs.row.tagged && (
            <p className={styles.caution} data-role="review-untagged">
              Line types are missing for this session. Run <code>shadowrunr review tag {id}</code>, then reload; lines show as untagged until then.
            </p>
          )}
        </div>
        {tab === "timeline" ? <Timeline {...props} /> : tab === "types" ? <ReviewByType {...props} /> : tab === "player" ? <Player {...props} /> : tab === "judge" ? <Judge {...props} /> : tab === "moderation" ? <Moderation {...props} /> : <EngineCalls {...props} />}
      </div>
    </>
  );
}
