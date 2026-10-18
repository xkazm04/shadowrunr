// Layer 2 of Segments & ranks: the winner's three tabs (l2b.js P3.L2.segments) as routes:
//   segments/tables[/<SEG>]   segments/commands   segments/ranks[/<report id>]
import { pageRoute, RouteLink, useRouter } from "../../router";
import { StatePanel, Tabs, type TabItem } from "../../ui";
import { CommandsPage } from "./Commands";
import styles from "./Layer2.module.css";
import { RanksPage } from "./Ranks";
import { TablesPage } from "./Tables";

export type TabId = "tables" | "commands" | "ranks";

export const TABS: readonly TabItem<TabId>[] = [
  { id: "tables", label: "Segment tables" },
  { id: "commands", label: "Chat commands" },
  { id: "ranks", label: "Ranks and weights" },
];

const isTab = (s: string | undefined): s is TabId => TABS.some((t) => t.id === s);

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const router = useRouter();
  const [head, second, ...rest] = sub;
  const tab = isTab(head) ? head : undefined;
  // tables and ranks take one optional id; commands takes none.
  const known = tab !== undefined && rest.length === 0 && (tab !== "commands" || second === undefined);
  return (
    <div className={styles.root} data-page="segments" data-role="segments-layer2">
      <Tabs items={TABS} value={tab ?? "tables"} label="Segments & ranks" onSelect={(id) => router.navigate(pageRoute("segments", id))} />
      {!known ? (
        <>
          <StatePanel state="invalid" title={`There is no page at ${sub.join(" / ")}`} hint="The segment tables, the chat commands and the ranks are the pages here." />
          <RouteLink to={pageRoute("segments", "tables")} className={styles.secondary}>
            Open the segment tables
          </RouteLink>
        </>
      ) : tab === "tables" ? (
        <TablesPage focus={second} />
      ) : tab === "commands" ? (
        <CommandsPage />
      ) : (
        <RanksPage reportId={second} />
      )}
    </div>
  );
}
