// The Simulator poster's face motif (the winner's art.js `M.simulator`) re-set for the letterbox
// bands around A/1's scene. The winner prints a 5 x 9 field of dots over a core on a 200 x 400
// portrait face; a band is a wide strip, so the field's last three rows (6-8) close the top band
// and the core rises from under the scene into the bottom band. Both keep the winner's scale
// (200 units across the band's width, as across the poster face), its dot sizes and opacities
// (r = 4 + (7x + 13y) mod 9, opacity .35 + ((x + y) mod 4) x .18), and crop, never stretch.
import styles from "./BandArt.module.css";

const COLS = [0, 1, 2, 3, 4] as const;
const ROWS = [6, 7, 8] as const;

/** The winner's dot field, its last row 20 units above the band's lower edge. */
export function SimBandTop() {
  return (
    <svg className={styles.art} viewBox="0 0 200 40" preserveAspectRatio="xMidYMax slice" data-role="simulator-band-art">
      <g className={styles.t} data-role="simulator-band-dots">
        {ROWS.flatMap((y) =>
          COLS.map((x) => <circle key={`${x}.${y}`} cx={24 + x * 38} cy={20 - (8 - y) * 40} r={4 + ((x * 7 + y * 13) % 9)} opacity={(0.35 + ((x + y) % 4) * 0.18).toFixed(2)} />),
        )}
      </g>
    </svg>
  );
}

/** The winner's core (r 46 overprint, r 20 key), centred on the band's upper edge. */
export function SimBandBottom() {
  return (
    <svg className={styles.art} viewBox="0 0 200 40" preserveAspectRatio="xMidYMin slice">
      <circle className={`${styles.o} ${styles.a}`} cx="100" cy="0" r="46" />
      <circle className={styles.k} cx="100" cy="0" r="20" data-role="simulator-band-core" />
    </svg>
  );
}
