// characters/<id>/package/<tab>: one character's package, read once with character.get, under
// the winner's six tabs.
import type { CharacterDetail } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useRpc } from "../../data";
import { RouteLink, useRouter } from "../../router";
import { Tabs } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { FoilsTab, TonesTab } from "./CastTabs";
import { BibleTab, ConstitutionTab } from "./DocTabs";
import styles from "./Layer2.module.css";
import { allRoute, PKG_TABS, pkgRoute, type PkgTab } from "./routes";
import { RulesTab } from "./RulesTab";
import { SituationsTab } from "./SituationsTab";

export function PackagePage({ id, tab, situation }: { readonly id: string; readonly tab: PkgTab; readonly situation: string | undefined }) {
  const router = useRouter();
  const detail = useRpc("character.get", { id });
  return (
    <div data-role="characters-package" data-tab={tab}>
      <Tabs items={PKG_TABS} value={tab} label={`${detail.data?.name ?? id} · the package`} onSelect={(t) => router.navigate(pkgRoute(id, t))} />
      <div className={styles.leadRow}>
        <RouteLink to={allRoute()} className={styles.lead}>
          All characters
        </RouteLink>
      </div>
      {detail.data === undefined && detail.error?.code === "not_found" ? (
        <EmptyState title={`No character package "${id}"`} hint="Open All characters for the packages the daemon knows." />
      ) : (
        <QueryView query={detail}>{(d) => <Body d={d} tab={tab} situation={situation} />}</QueryView>
      )}
    </div>
  );
}

function Body({ d, tab, situation }: { d: CharacterDetail; tab: PkgTab; situation: string | undefined }) {
  switch (tab) {
    case "constitution":
      return <ConstitutionTab d={d} />;
    case "rules":
      return <RulesTab d={d} />;
    case "bible":
      return <BibleTab d={d} />;
    case "tones":
      return <TonesTab d={d} />;
    case "foils":
      return <FoilsTab d={d} />;
    case "situations":
      return <SituationsTab d={d} selected={situation} />;
  }
}
