// The On Air poster face (the winner's art.js `M.onair`): a sun of three rings over a beam and a
// screen. Fills come from the page palette, so the face prints in On Air's colours.
import { useRpc } from "../../data";
import styles from "./Motif.module.css";

export function Motif() {
  return (
    <svg className={styles.motif} viewBox="0 0 200 400" preserveAspectRatio="xMidYMid slice" aria-hidden="true" data-role="onair-motif">
      <circle className={`${styles.o} ${styles.a}`} cx="100" cy="116" r="84" />
      <circle className={styles.t} cx="100" cy="116" r="52" opacity=".9" />
      <circle className={styles.f} cx="100" cy="116" r="30" />
      <path className={styles.k} d="M100 116L12 400H188Z" opacity=".16" />
      <rect className={styles.k} x="34" y="262" width="132" height="84" rx="3" />
      <rect className={styles.o2} x="42" y="270" width="116" height="68" opacity=".9" />
      <path className={`${styles.k} ${styles.lines}`} d="M42 284H158M42 296H158M42 308H158M42 320H158" opacity=".35" strokeWidth="2" />
      <circle className={`${styles.o} ${styles.b}`} cx="150" cy="364" r="9" />
    </svg>
  );
}

/** Glance chips on the hovered wall poster: what plays, and the latest run's tone. */
export function Glance() {
  const runs = useRpc("runs.page", { limit: 1 });
  const tone = runs.data?.items[0]?.spec.tone_variant.replace(/-/g, " ");
  return (
    <>
      <b>Live replay</b>
      {tone !== undefined && <b>{tone}</b>}
    </>
  );
}
