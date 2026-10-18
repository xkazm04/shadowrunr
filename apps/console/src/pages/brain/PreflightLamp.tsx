// Layer 1's pre-flight lamp, on the poster's brand row: the worst state of the latest
// `preflight.run` (expected media ambers do not count), a door into brain/preflight. The brand
// row has a fixed height, so the lamp moves none of the fidelity roles.
import { useRpc } from "../../data";
import { RouteLink, pageRoute } from "../../router";
import { PREFLIGHT_PARAMS, worstState } from "./preflightModel";
import styles from "./Layer1.module.css";

const PREFLIGHT = pageRoute("brain", "preflight");

export function PreflightLamp() {
  const q = useRpc("preflight.run", PREFLIGHT_PARAMS);
  const state = q.data === undefined ? "unknown" : worstState(q.data);
  const words = state === "unknown" ? (q.errorView ? "not read" : "checking") : state;
  return (
    <RouteLink to={PREFLIGHT} className={styles.pf} data-role="brain-door-preflight" aria-label={`Pre-flight: ${words}. Open the pre-flight board`}>
      <span className={styles.pfLamp} data-state={state} data-role="brain-preflight-lamp" aria-hidden="true" />
      Pre-flight · {words}
    </RouteLink>
  );
}
