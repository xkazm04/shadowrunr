// Layer 2 of Characters (the winner's l2.js P3.L2.characters) as routes: all characters, then one
// character's package with its eight tabs.
import { RouteLink } from "../../router";
import { buttonClass } from "../../ui/Button";
import { StatePanel } from "../../ui/States";
import { AllPage } from "./AllPage";
import styles from "./Layer2.module.css";
import { PackagePage } from "./PackagePage";
import { allRoute, readSub } from "./routes";

export function Layer2({ sub }: { readonly sub: readonly string[] }) {
  const r = readSub(sub);
  return (
    <div className={styles.root} data-page="characters" data-role="characters-layer2">
      {r.kind === "all" ? (
        <AllPage />
      ) : r.kind === "package" ? (
        <PackagePage id={r.id} tab={r.tab} situation={r.situation} from={r.from} />
      ) : (
        <>
          <StatePanel state="invalid" title={`There is no page at ${sub.join(" / ")}`} hint="All characters, and each character's package (constitution, optional rules, bible, tones, foils, situations, parameters, voice, Tone Lab), are the pages here." />
          <RouteLink to={allRoute()} className={buttonClass("secondary", "compact")}>
            All characters
          </RouteLink>
        </>
      )}
    </div>
  );
}
