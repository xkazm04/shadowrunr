// Brain layer 2: `engine` (routing, fleet accounts, fallbacks, the call ledger),
// `transcripts[/<run>[/<line>]]` (the run list, then the transcript viewer with line deep links)
// and `preflight` (the PF-01..PF-09 board that gates every stream start).
import { pageRoute, useRouter } from "../../router";
import { Head, Tabs } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { Engine } from "./Engine";
import { Preflight } from "./Preflight";
import { Runs } from "./Runs";
import { Viewer } from "./Viewer";
import styles from "./Layer2.module.css";

type Floor = "engine" | "transcripts" | "preflight";
const FLOORS = [
  { id: "engine", label: "The engine" },
  { id: "transcripts", label: "Transcripts" },
  { id: "preflight", label: "Pre-flight" },
] as const satisfies readonly { id: Floor; label: string }[];

const isFloor = (s: string | undefined): s is Floor => FLOORS.some((f) => f.id === s);

function titleOf(floor: Floor, run: string | undefined): string {
  if (floor === "engine") return "The engine";
  if (floor === "preflight") return "Pre-flight";
  return run === undefined ? "Transcripts" : "Transcript";
}

function ledeOf(floor: Floor, run: string | undefined): string {
  if (floor === "engine") return "Routing, fleet accounts, fallbacks and every call in the ledger.";
  if (floor === "preflight") return "The health check that gates every stream start: a red check refuses the start. Open a row for its detail.";
  return run === undefined ? "Every stream the simulator logged. Pick a run to read its transcript line by line." : run;
}

/** `transcripts/<run>/<line>`: the 1-based line, when it is one. */
export function lineOf(seg: string | undefined): number | undefined {
  if (seg === undefined || !/^\d+$/.test(seg)) return undefined;
  const n = Number(seg);
  return n >= 1 ? n : undefined;
}

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const router = useRouter();
  const [floor, run, line] = sub;
  return (
    <div className={styles.page} data-role="brain-layer2">
      <Head kicker="Layer 2 · Brain floor" title={isFloor(floor) ? titleOf(floor, run) : "The Brain"} lede={isFloor(floor) ? ledeOf(floor, run) : undefined} />
      {isFloor(floor) && <Tabs label="Brain floors" items={FLOORS} value={floor} onSelect={(id) => router.navigate(pageRoute("brain", id))} />}
      {floor === "engine" && <Engine />}
      {floor === "transcripts" && run === undefined && <Runs />}
      {floor === "transcripts" && run !== undefined && <Viewer run={run} line={lineOf(line)} />}
      {floor === "preflight" && <Preflight />}
      {!isFloor(floor) && <EmptyState title={`No brain page called “${floor ?? ""}”`} hint="Open the engine, the transcripts or the pre-flight board from the doors on layer 1." />}
    </div>
  );
}
