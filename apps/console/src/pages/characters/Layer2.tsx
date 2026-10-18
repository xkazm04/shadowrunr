// Layer 2 of Characters (the winner's l2.js P3.L2.characters) as routes: all characters, then one
// character's package with its six tabs.
import { RouteLink } from "../../router";
import { StatePanel } from "../../ui";
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
        <PackagePage id={r.id} tab={r.tab} situation={r.situation} />
      ) : (
        <>
          <StatePanel state="invalid" title={`There is no page at ${sub.join(" / ")}`} hint="All characters, and each character's package (constitution, optional rules, bible, tones, foils, situations), are the pages here." />
          <RouteLink to={allRoute()} className={styles.lead}>
            All characters
          </RouteLink>
        </>
      )}
    </div>
  );
}
