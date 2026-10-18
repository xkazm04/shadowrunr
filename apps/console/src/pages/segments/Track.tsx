// A/1's segment lifecycle track (app.js SCENES.segments), printed in the poster's inks: one
// station per lifecycle column on a rail, lit where segments stand, and the descoped siding
// that leaves the rail before the pilot station. Descoped tables (SEG-02B) never ran: the
// runtime refuses to start them, so they sit on the siding, never in the retired column.
import type { SegmentTableInfo } from "@shadowrunr/contract";
import styles from "./Layer1.module.css";
import { descoped, DESCOPED_SHOWN, LIFECYCLE, statusCounts } from "./model";

const HEX = "0,-28 25,-14 25,14 0,28 -25,14 -25,-14";

export function Track({ segments }: { segments: readonly SegmentTableInfo[] }) {
  const counts = statusCounts(segments);
  const off = descoped(segments);
  const shown = off.slice(0, DESCOPED_SHOWN);
  const more = off.length - shown.length;
  return (
    <div className={styles.track} data-role="segments-track">
      {LIFECYCLE.map((s) => (
        <div key={s} className={styles.station} data-role="segments-station" data-status={s} data-lit={counts[s] > 0}>
          <i className={styles.lamp} />
          <span className={styles.stationTag}>
            {s} · {counts[s]}
          </span>
        </div>
      ))}
      {off.length > 0 && (
        <div className={styles.siding} data-role="segments-descoped">
          <svg className={styles.spur} viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
            <path d="M0 0 Q30 40 100 36" />
          </svg>
          {shown.map((s, i) => (
            <svg key={`${s.id}:${i}`} className={styles.hex} viewBox="-30 -30 60 60" role="img" aria-label={`${s.id} ${s.title}: descoped`} data-role="segments-descoped-hex">
              <polygon points={HEX} />
              <text y="5" textAnchor="middle" fontSize="15">
                {s.id.replace("SEG-", "")}
              </text>
            </svg>
          ))}
          {more > 0 && <span className={styles.sidingMore}>+{more.toLocaleString("en-US")}</span>}
          <span className={styles.sidingTag}>descoped · {off.length} · the runtime refuses to start them</span>
        </div>
      )}
    </div>
  );
}
