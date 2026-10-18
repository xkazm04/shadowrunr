// Layer 2, Simulator: the winner's nested pages (l2b.js `P3.L2.simulator`), tabbed:
// tournaments[/<id>], runs, verdicts[/<run>], heatmap[/<key>].
import { pageRoute, useRouter } from "../../router";
import { Tabs, type TabItem } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { HeatmapPage } from "./Heatmap";
import styles from "./l2.module.css";
import { RunsPage } from "./Runs";
import { TournamentDetail, TournamentList } from "./Tournaments";
import { VerdictDetail, VerdictPicker } from "./Verdicts";

export type SimTab = "tournaments" | "runs" | "verdicts" | "heatmap";

export const TABS: readonly TabItem<SimTab>[] = [
  { id: "tournaments", label: "Tournaments" },
  { id: "runs", label: "Runs" },
  { id: "verdicts", label: "Verdicts" },
  { id: "heatmap", label: "Heatmap" },
];

const isTab = (s: string | undefined): s is SimTab => TABS.some((t) => t.id === s);

function Body({ tab, arg }: { tab: SimTab; arg: string | undefined }) {
  switch (tab) {
    case "tournaments":
      return arg === undefined ? <TournamentList /> : <TournamentDetail id={arg} />;
    case "runs":
      return <RunsPage />;
    case "verdicts":
      return arg === undefined ? <VerdictPicker /> : <VerdictDetail runId={arg} />;
    case "heatmap":
      return <HeatmapPage cellKey={arg} />;
  }
}

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const router = useRouter();
  const [seg, arg, extra] = sub;
  const tab = isTab(seg) ? seg : undefined;
  return (
    <div className={styles.l2} data-role="simulator-l2">
      <span className={styles.sr}>Layer 2 · The Trials</span>
      <Tabs items={TABS} value={tab ?? "tournaments"} onSelect={(id) => router.navigate(pageRoute("simulator", id))} label="Simulator pages" />
      {tab === undefined || extra !== undefined ? (
        <EmptyState title="There is no such simulator page" hint="Pick Tournaments, Runs, Verdicts or Heatmap above." />
      ) : (
        <Body key={tab} tab={tab} arg={arg} />
      )}
    </div>
  );
}
