// On Air, layer 2: the winner's two nested pages, `rundown` and `dispatch`, behind its tabs.
//   #/onair/rundown                         every run, paged and virtualised; the segment cards
//   #/onair/rundown/<tournament>/<run>      one run's segment spans and beats, and "replay it"
//   #/onair/dispatch[/<report>]             the Lord's dispatch, from a report's plan options
import { Head, Tabs } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import { pageRoute, useRouter } from "../../router";
import { Dispatch } from "./Dispatch";
import styles from "./Layer2.module.css";
import { Rundown } from "./Rundown";

type Tab = "rundown" | "dispatch";
const TABS = [
  { id: "rundown", label: "Rundown" },
  { id: "dispatch", label: "The Lord’s dispatch" },
] as const satisfies readonly { id: Tab; label: string }[];

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const router = useRouter();
  const [tab, a, b] = sub;
  const known = tab === "rundown" || tab === "dispatch";
  return (
    <div className={styles.l2} data-page="onair" data-role="onair-layer2">
      <Tabs items={TABS} value={known ? tab : "rundown"} onSelect={(id) => router.navigate(pageRoute("onair", id))} label="On Air sections" />
      {tab === "rundown" && (
        <>
          <Head
            kicker="Layer 2 · On Air"
            title="The rundown"
            lede="The segments of the night, and the moments between them. Handover is a clock, not a mood: the brain only emits switch_segment when the clock allows it."
          />
          <Rundown run={a !== undefined && b !== undefined ? { tournamentId: a, runId: b } : null} />
        </>
      )}
      {tab === "dispatch" && (
        <>
          <Head
            kicker="Layer 2 · On Air"
            title="The Lord’s dispatch"
            lede="Each night’s plan arrives as a dispatch. Malakor never names or describes the Lord, and never claims the Lord is watching live."
          />
          <Dispatch reportId={a ?? null} />
        </>
      )}
      {!known && (
        <>
          <Head kicker="Layer 2 · On Air" title="Not a page" />
          <StatePanel state="invalid" title={`On Air has no page “${tab ?? ""}”`} hint="Pick the rundown or the Lord’s dispatch above." />
        </>
      )}
    </div>
  );
}
