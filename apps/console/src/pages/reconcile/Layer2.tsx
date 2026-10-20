// Layer 2 of Reconcile & Plan: the winner's four tabs (l2b.js P3.L2.reconcile) as routes:
//   reconcile/report[/held|failed]   reconcile/plan   reconcile/directives   reconcile/chat
import { pageRoute, RouteLink, useRouter } from "../../router";
import { buttonClass } from "../../ui/Button";
import { Tabs, type TabItem } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import { ChatPage } from "./ChatPage";
import { DirectivesPage } from "./DirectivesPage";
import { useReportId } from "./hooks";
import styles from "./Layer2.module.css";
import { NoReport } from "./NoReport";
import { PlanPage } from "./PlanPage";
import { ReportPage, type ReportFilter } from "./ReportPage";

export type TabId = "report" | "plan" | "directives" | "chat";

export const TABS: readonly TabItem<TabId>[] = [
  { id: "report", label: "Report" },
  { id: "plan", label: "Plan options" },
  { id: "directives", label: "Directives" },
  { id: "chat", label: "The Lord’s chat" },
];

const isTab = (s: string | undefined): s is TabId => TABS.some((t) => t.id === s);

function filterOf(seg: string | undefined): ReportFilter | undefined {
  if (seg === undefined) return "all";
  return seg === "held" || seg === "failed" ? seg : undefined;
}

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const router = useRouter();
  const [head, second, ...rest] = sub;
  const tab = isTab(head) ? head : undefined;
  return (
    <div className={styles.root} data-page="reconcile" data-role="reconcile-layer2">
      <Tabs items={TABS} value={tab ?? "report"} label="Reconcile & plan" onSelect={(id) => router.navigate(pageRoute("reconcile", id))} />
      <Body tab={tab} second={second} extra={rest.length > 0} sub={sub} />
    </div>
  );
}

function Body({ tab, second, extra, sub }: { tab: TabId | undefined; second: string | undefined; extra: boolean; sub: readonly string[] }) {
  const { reportId, pending } = useReportId();
  const filter = tab === "report" && !extra ? filterOf(second) : undefined;
  const known = tab !== undefined && !extra && (tab === "report" ? filter !== undefined : second === undefined);
  if (!known) {
    return (
      <>
        <StatePanel state="invalid" title={`There is no page at ${sub.join(" / ")}`} hint="The report, plan options, directives and the Lord’s chat are the pages here." />
        <RouteLink to={pageRoute("reconcile", "report")} className={buttonClass("secondary")}>
          Open the report
        </RouteLink>
      </>
    );
  }
  if (pending && (tab === "report" || tab === "plan")) return <StatePanel state="loading" title="Finding the newest report" />;
  switch (tab) {
    case "report":
      return reportId === undefined ? <NoReport /> : <ReportPage reportId={reportId} filter={filter ?? "all"} />;
    case "plan":
      return reportId === undefined ? <NoReport /> : <PlanPage reportId={reportId} />;
    case "directives":
      return <DirectivesPage />;
    case "chat":
      return <ChatPage reportId={reportId} />;
  }
}
