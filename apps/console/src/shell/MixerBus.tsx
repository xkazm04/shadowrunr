import type { CSSProperties } from "react";
import { PAGE_IDS, RouteLink, pageRoute, type PageId } from "../router";
import styles from "./MixerBus.module.css";
import { POSTERS } from "./posters";
import type { PageRegistry } from "./types";

/** Bus heights in rem; the layout reserves this much below the posters. */
export const BUS_REM = { full: 4.2, compact: 2.6 } as const;

export function MixerBus(props: { pages: PageRegistry; onAir: PageId; compact: boolean; narrow: boolean }) {
  const height = props.compact || props.narrow ? BUS_REM.compact : BUS_REM.full;
  const style = { "--bus-height": `${height}rem` } as CSSProperties;
  return (
    <nav
      className={styles.bus}
      data-compact={props.compact}
      data-narrow={props.narrow}
      data-role="mixer-bus"
      aria-label="Vision mixer: six pages, keys 1 to 6"
      style={style}
    >
      <div className={styles.label}>
        <b>Vision mixer</b>
        keys 1–6 · tally = on air
      </div>
      {PAGE_IDS.map((id, i) => {
        const Mini = props.pages[id].Mini;
        const onAir = id === props.onAir;
        return (
          <RouteLink
            key={id}
            to={pageRoute(id)}
            className={styles.channel}
            aria-label={`Camera ${i + 1}: ${POSTERS[id].name}${onAir ? " (on air)" : ""}`}
            aria-current={onAir ? "page" : undefined}
            data-role="bus-channel"
            data-page={id}
          >
            <span className={styles.mini} aria-hidden="true">
              {Mini ? <Mini /> : <span className={styles.slate}>{i + 1}</span>}
            </span>
            <span className={styles.name}>
              <small>Cam {i + 1}</small>
              {POSTERS[id].name}
            </span>
            <span className={styles.tally} aria-hidden="true" />
          </RouteLink>
        );
      })}
    </nav>
  );
}
