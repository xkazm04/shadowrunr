// The Brain poster's face motif (the winner's art.js `M.brain`) re-set for the letterbox bands
// around A/1's scene. The winner prints it on a 200 x 400 portrait face; a band is a wide strip,
// so the motif is split at the scene: the rings radiate from behind it into the top band and the
// pulse runs under it in the bottom band. Both keep the winner's scale (200 units across the
// band's width, as across the poster face) and crop, never stretch, when a band grows taller.
// Colours are the page palette, at the winner's active-poster motif opacity.
import styles from "./BandArt.module.css";

/** The winner's rings (r 30, 58, 88, 118), centred below the band's lower edge. */
export function BrainBandTop() {
  return (
    <svg className={styles.art} viewBox="0 0 200 40" preserveAspectRatio="xMidYMax slice" data-role="brain-band-art">
      <g className={styles.rings} strokeWidth="3" data-role="brain-band-rings">
        <circle className={styles.a} cx="100" cy="60" r="30" />
        <circle className={styles.b} cx="100" cy="60" r="58" opacity=".8" />
        <circle className={styles.a} cx="100" cy="60" r="88" opacity=".6" />
        <circle cx="100" cy="60" r="118" opacity=".4" />
      </g>
    </svg>
  );
}

/**
 * The winner's pulse (M0 300H44L58 270 74 336 92 250 108 350 124 290 138 306H200) with its
 * amplitude set to 0.3 about a baseline at 12, so it clips at a short band's edges like a trace
 * on a scope; then the winner's dot row.
 */
export function BrainBandBottom() {
  return (
    <svg className={styles.art} viewBox="0 0 200 40" preserveAspectRatio="xMidYMin slice">
      <path className={styles.pulse} d="M0 12H44L58 3 74 22.8 92 -3 108 27 124 9 138 13.8H200" strokeWidth="5" strokeLinejoin="round" data-role="brain-band-pulse" />
      <g className={styles.dots} opacity=".7">
        {[30, 62, 94, 126, 158].map((x) => (
          <circle key={x} cx={x} cy="34" r="4" />
        ))}
      </g>
    </svg>
  );
}
