// Pieces of the On Air screen that change per event: the chat lines, the weighted vote, the
// kinetic caption and the decree seal. Each is a small memoised component over view.ts data.
import { memo, type CSSProperties } from "react";
import styles from "./Screen.module.css";
import type { CaptionView, ChatRow, Life, VoteView } from "./view";

/** CSS custom properties for a timed overlay: how far into its life it already is. */
export function lifeStyle(life: Life): CSSProperties {
  return { "--d": `${life.delayMs}ms` } as CSSProperties;
}

export const ChatLines = memo(function ChatLines({ rows }: { rows: readonly ChatRow[] }) {
  return (
    <div className={styles.chL}>
      {rows.map((m) => (
        <div
          key={m.id}
          className={styles.msg}
          data-rank={m.rank}
          data-cmd={m.command || undefined}
          data-picked={m.picked || undefined}
          data-redact={m.redacted || undefined}
        >
          <i title={m.rank} />
          <b>{m.name}</b> <span>{m.text}</span>
        </div>
      ))}
    </div>
  );
});

export const VoteRows = memo(function VoteRows({ vote }: { vote: VoteView | null }) {
  return (
    <>
      <div className={styles.vtQ}>{vote?.question ?? ""}</div>
      <div className={styles.vtRows}>
        {vote?.rows.map((o) => (
          <div key={o.name} className={styles.vrow} data-win={o.win || undefined}>
            <div className={styles.vlab}>
              <span>{o.name}</span>
              <em>{o.weight}</em>
            </div>
            <div className={styles.vb}>
              <i style={{ width: `${o.pct}%` }} />
            </div>
          </div>
        ))}
      </div>
    </>
  );
});

/** Words land one by one, 70 ms apart (the winner's caption), even when mounted late. */
export const CaptionText = memo(function CaptionText({ caption }: { caption: CaptionView | null }) {
  if (!caption) return <div className={styles.cpT} aria-live="off" />;
  const late = caption.talk.delayMs / 1000;
  return (
    <div key={caption.key} className={styles.cpT} data-foil={caption.foil || undefined} aria-live="off" data-role="onair-caption">
      {caption.words.map((w, i) => (
        <span key={i} className={styles.w} style={{ animationDelay: `${Math.max(late + i * 0.07, late).toFixed(2)}s` }}>
          {w.aside ? <i>{w.text}</i> : w.text}{" "}
        </span>
      ))}
    </div>
  );
});

/** The decree seal: an ember disc with a black star (stage.js, kept exactly). */
export function DecreeSeal() {
  return (
    <svg viewBox="0 0 60 60" aria-hidden="true">
      <circle cx="30" cy="30" r="27" className={styles.sealDisc} strokeWidth="3" />
      <path d="M30 12l5 12 13 1-10 8 4 13-12-7-12 7 4-13-10-8 13-1z" className={styles.sealStar} />
    </svg>
  );
}

/** The set behind the stream: three arches and three candle flames (stage.js `scr-bg`). */
export const ScreenSet = memo(function ScreenSet() {
  return (
    <div className={styles.scrBg} data-role="onair-set">
      <svg viewBox="0 0 160 90" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
        <path className={styles.a1} d="M4 90V38Q4 14 28 12T52 38V90Z" />
        <path className={styles.a2} d="M60 90V30Q60 6 84 4T108 30V90Z" />
        <path className={styles.a1} d="M116 90V40Q116 16 140 14T164 40V90Z" />
        <circle className={styles.cd} cx="30" cy="70" r="9" />
        <circle className={styles.cd} cx="132" cy="74" r="8" />
        <circle className={styles.cd} cx="84" cy="60" r="7" />
      </svg>
    </div>
  );
});
