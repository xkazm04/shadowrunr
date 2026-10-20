// Layer 2 of Session review: one sandbox session and its tabs, as routes:
//   review/<session>[/timeline[/<line>]]   review/<session>/types   review/<session>/player/<kind>
//   review/<session>/judge   review/<session>/moderation   review/<session>/calls
import type { ReviewSession } from "@shadowrunr/contract";
import { QueryView } from "../../kit/QueryView";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { Head, Tabs, type TabItem } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import { EngineCalls } from "./EngineCalls";
import { useDerived, useFeedback, useSession } from "./hooks";
import { Judge } from "./Judge";
import styles from "./Layer2.module.css";
import { Moderation } from "./Moderation";
import { CHECK_NAME, fmtClock, KIND_NAME, type ShownKind } from "./names";
import { Player } from "./Player";
import { ReviewByType } from "./ReviewByType";
import type { TabId, TabProps } from "./tab";
import { Timeline } from "./Timeline";

const TAB_IDS: readonly TabId[] = ["timeline", "types", "player", "judge", "moderation", "calls"];
const isTab = (s: string | undefined): s is TabId => TAB_IDS.includes(s as TabId);
const isKind = (s: string | undefined): s is ShownKind => s !== undefined && s in KIND_NAME;

export function sessionTitle(minutes: number, chaos?: string): string {
  return `${minutes}-minute session${chaos === "lost" ? " · engine lost" : chaos === "outage" ? " · engine outage" : ""}`;
}

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const [sessionId, head, arg, ...rest] = sub;
  const tab: TabId = isTab(head) ? head : "timeline";
  const known = sessionId !== undefined && (head === undefined || isTab(head)) && rest.length === 0 && (tab !== "player" || isKind(arg));
  const query = useSession(known ? sessionId : undefined);
  return (
    <div className={styles.root} data-page="review" data-role="review-layer2">
      {!known ? (
        <>
          <StatePanel state="invalid" title={`There is no page at ${sub.join(" / ")}`} hint="Pick a session on the wall; each has a timeline, a review by type, the judge, moderation and engine calls." />
          <RouteLink to={pageRoute("review")} className={styles.secondary}>
            Back to all sessions
          </RouteLink>
        </>
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
  return (
    <>
      <div className={styles.head}>
        <Head
          kicker={`${id} · ${fmtClock(d.lengthS)} of stream · ${d.says.length} lines${rs.row.tagged ? "" : " · not tagged yet"}`}
          title={
            <>
              {sessionTitle(rs.row.minutes, rs.row.chaos)} <span className={rs.row.pass ? styles.pass : styles.fail}>{rs.row.pass ? "passed" : "failed"}</span>
            </>
          }
        />
        <ul className={styles.gates} aria-label="Gate checks">
          {rs.row.checks.map((c) => (
            <li key={c.id} className={styles[c.state]} title={c.detail}>
              {c.state === "pass" ? "✓" : c.state === "fail" ? "✗" : "–"} {CHECK_NAME[c.id] ?? c.id}
            </li>
          ))}
        </ul>
      </div>
      {!rs.row.tagged && (
        <StatePanel state="degraded" title="Line types are missing for this session" hint={`Run shadowrunr review tag ${id}, then reload. Lines show as untagged until then.`} />
      )}
      <Tabs items={items} value={tab} label="Session review" onSelect={(t) => router.navigate(pageRoute("review", id, ...(t === "player" && kind ? ["player", kind] : [t])))} />
      {tab === "timeline" ? <Timeline {...props} /> : tab === "types" ? <ReviewByType {...props} /> : tab === "player" ? <Player {...props} /> : tab === "judge" ? <Judge {...props} /> : tab === "moderation" ? <Moderation {...props} /> : <EngineCalls {...props} />}
    </>
  );
}
