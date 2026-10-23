// characters/<id>/package/<tab>: one character's package, read once with character.get, under
// the winner's six tabs, the v2 parameters, the Voice tab and the Tone Lab, plus the Moderation
// tab on the moderator character's package only (moderator.status names it; an error hides it).
import { lazy, Suspense } from "react";
import type { CharacterDetail } from "@shadowrunr/contract";
import { QueryView } from "../../kit";
import { useRpc } from "../../data";
import { RouteLink, useRouter } from "../../router";
import { buttonClass } from "../../ui/Button";
import { Tabs } from "../../ui/Section";
import { EmptyState, StatePanel } from "../../ui/States";
import { FoilsTab, TonesTab } from "./CastTabs";
import { BibleTab, ConstitutionTab } from "./DocTabs";
import styles from "./Layer2.module.css";
import { ParamsTab } from "./ParamsTab";
import { allRoute, PKG_TABS, pkgRoute, type PkgTab, type ReviewRef } from "./routes";
import { RulesTab } from "./RulesTab";
import { SituationsTab } from "./SituationsTab";
import { ToneLab } from "./ToneLab";
import { VoiceTab } from "./VoiceTab";

// Its own named chunk (characters.moderation in scripts/bundle-measure.mjs): host packages never load it.
const ModerationTab = lazy(() => import("./moderation/ModerationTab").then((m) => ({ default: m.ModerationTab })));

export function PackagePage({ id, tab, situation, from }: { readonly id: string; readonly tab: PkgTab; readonly situation: string | undefined; readonly from?: ReviewRef | undefined }) {
  const router = useRouter();
  const detail = useRpc("character.get", { id });
  const mod = useRpc("moderator.status", {});
  const isModerator = mod.data?.character === id;
  const tabs = isModerator ? PKG_TABS : PKG_TABS.filter((t) => t.id !== "moderation");
  return (
    <div data-role="characters-package" data-tab={tab} data-rise>
      <Tabs items={tabs} value={tab} label={`${detail.data?.name ?? id} · the package`} onSelect={(t) => router.navigate(pkgRoute(id, t))} />
      <div className={styles.leadRow}>
        <RouteLink to={allRoute()} className={buttonClass("secondary", "compact")}>
          All characters
        </RouteLink>
      </div>
      {detail.data === undefined && detail.error?.code === "not_found" ? (
        <EmptyState title={`No character package "${id}"`} hint="Open All characters for the packages the daemon knows." />
      ) : (
        <QueryView query={detail}>{(d) => <Body d={d} tab={tab} situation={situation} from={from} moderator={isModerator ? true : mod.data !== undefined || mod.error !== undefined ? false : undefined} />}</QueryView>
      )}
    </div>
  );
}

/** `moderator`: this character is the moderator; undefined while moderator.status is unanswered. */
function Body({ d, tab, situation, from, moderator }: { d: CharacterDetail; tab: PkgTab; situation: string | undefined; from: ReviewRef | undefined; moderator: boolean | undefined }) {
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
    case "parameters":
      return <ParamsTab d={d} />;
    case "voice":
      return <VoiceTab d={d} />;
    case "tone-lab":
      return <ToneLab d={d} from={from} />;
    case "moderation":
      if (moderator === undefined) return <StatePanel state="loading" title="Asking the daemon for the moderator" />;
      if (!moderator) return <EmptyState title={`${d.name} is not the moderator`} hint="The Moderation tab belongs to the package the daemon's [moderator] character names." />;
      return (
        <Suspense fallback={<StatePanel state="loading" title="Loading" />}>
          <ModerationTab d={d} />
        </Suspense>
      );
  }
}
