// Brain layer 2: `engine` (routing, fleet accounts, fallbacks, the call ledger),
// `transcripts[/<run>[/<line>]]` (the run list, then the transcript viewer with line deep links)
// and `preflight` (the PF-01..PF-09 board that gates every stream start).
import { useRouter } from "../../router";
import { BRAIN_ROUTES, FLOORS, floorRoute, type Floor } from "../../router/brain";
import { matchSub } from "../../router/schema";
import { Head, Tabs } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { Engine } from "./Engine";
import { Preflight } from "./Preflight";
import { Runs } from "./Runs";
import { Viewer } from "./Viewer";
import styles from "./Layer2.module.css";

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
  // The shell replaces a sub the schema rejects with its accepted prefix before mounting this;
  // mounted directly, the same prefix is read, and an unknown floor still says so.
  const m = matchSub(BRAIN_ROUTES, sub);
  const valid = m.ok ? sub : m.fallback;
  const read = m.ok ? m : matchSub(BRAIN_ROUTES, valid);
  const floor = valid[0];
  const { run, line } = read.ok ? read.params : {};
  return (
    <div className={styles.page} data-page="brain" data-role="brain-layer2">
      {isFloor(floor) && <Tabs label="Brain floors" items={FLOORS} value={floor} onSelect={(id) => router.navigate(floorRoute(id))} />}
      <Head kicker="Layer 2 · Brain floor" title={isFloor(floor) ? titleOf(floor, run) : "The Brain"} lede={isFloor(floor) ? ledeOf(floor, run) : undefined} />
      {floor === "engine" && <Engine />}
      {floor === "transcripts" && run === undefined && <Runs />}
      {floor === "transcripts" && run !== undefined && <Viewer run={run} line={lineOf(line)} />}
      {floor === "preflight" && <Preflight />}
      {!isFloor(floor) && <EmptyState title={`No brain page called “${sub.join("/")}”`} hint="Open the engine, the transcripts or the pre-flight board from the doors on layer 1." />}
    </div>
  );
}
