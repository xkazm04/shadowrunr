// Brain layer 2: `engine` (routing, fleet accounts, fallbacks, the call ledger) and
// `transcripts[/<run>[/<line>]]` (the run list, then the transcript viewer with line deep links).
import { pageRoute, useRouter } from "../../router";
import { EmptyState, Head, Tabs } from "../../ui";
import { Engine } from "./Engine";
import { Runs } from "./Runs";
import { Viewer } from "./Viewer";
import styles from "./Layer2.module.css";

type Floor = "engine" | "transcripts";
const FLOORS = [
  { id: "engine", label: "The engine" },
  { id: "transcripts", label: "Transcripts" },
] as const;

const isFloor = (s: string | undefined): s is Floor => s === "engine" || s === "transcripts";

/** `transcripts/<run>/<line>`: the 1-based line, when it is one. */
export function lineOf(seg: string | undefined): number | undefined {
  if (seg === undefined || !/^\d+$/.test(seg)) return undefined;
  const n = Number(seg);
  return n >= 1 ? n : undefined;
}

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const router = useRouter();
  const [floor, run, line] = sub;
  const lede = !isFloor(floor)
    ? undefined
    : floor === "engine"
      ? "Routing, fleet accounts, fallbacks and every call in the ledger."
      : run === undefined
        ? "Every stream the simulator logged. Pick a run to read its transcript line by line."
        : run;
  return (
    <div className={styles.page} data-role="brain-layer2">
      <Head kicker="Layer 2 · Brain floor" title={isFloor(floor) ? (floor === "engine" ? "The engine" : run === undefined ? "Transcripts" : "Transcript") : "The Brain"} lede={lede} />
      {isFloor(floor) && <Tabs label="Brain floors" items={FLOORS} value={floor} onSelect={(id) => router.navigate(pageRoute("brain", id))} />}
      {floor === "engine" && <Engine />}
      {floor === "transcripts" && run === undefined && <Runs />}
      {floor === "transcripts" && run !== undefined && <Viewer run={run} line={lineOf(line)} />}
      {!isFloor(floor) && <EmptyState title={`No brain page called “${floor ?? ""}”`} hint="Open the engine or the transcripts from the tabs on layer 1." />}
    </div>
  );
}
