import { useMemo } from "react";

import type { Caption as CaptionT, Redaction } from "../sceneModel";
import { captionOpacity, visibleWords, wordTimings } from "../timing";

/** Words of a line, revealed on estimated timing. All words are laid out from the start (hidden
 * ones are invisible) so the text never reflows while it is being spoken. */
export function KineticText({ text, elapsed, className }: { text: string; elapsed: number; className?: string }) {
  const line = useMemo(() => wordTimings(text), [text]);
  const shown = visibleWords(line, elapsed);
  return (
    <span className={className}>
      {line.words.map((w, i) => (
        <span key={i} className={`w${i < shown ? " in" : ""}${w.emphasis ? " em" : ""}`}>
          {w.word}{" "}
        </span>
      ))}
    </span>
  );
}

export function captionSize(text: string, compact: boolean): number {
  const n = text.length;
  if (compact) return n > 120 ? 21 : 26;
  return n > 150 ? 31 : n > 110 ? 35 : n > 70 ? 40 : 46;
}

export const REDACTION_MS = 3800;

export function CaptionPanel({
  caption,
  redactions,
  now,
  compact,
}: {
  caption: CaptionT | null;
  redactions: Redaction[];
  now: number;
  compact: boolean;
}) {
  const line = useMemo(() => (caption ? wordTimings(caption.text) : null), [caption]);
  const elapsed = caption ? now - caption.at : 0;
  const opacity = caption && line ? captionOpacity(line, elapsed) : 0;
  const redaction = [...redactions].reverse().find((r) => now >= r.at && now - r.at < REDACTION_MS);

  return (
    <div className={`caption-panel${compact ? " compact" : ""}`}>
      {caption && opacity > 0 && (
        <div className="caption" key={caption.id} style={{ opacity, fontSize: captionSize(caption.text, compact) }}>
          <div className="caption-head">
            <span className="speaker">MALAKOR</span>
            {caption.replyingTo.slice(0, 3).map((n) => (
              <span className="reply-chip" key={n}>
                ↳ {n}
              </span>
            ))}
          </div>
          <KineticText className="caption-text" text={caption.text} elapsed={elapsed} />
        </div>
      )}
      {redaction && (
        <div className="redaction" key={redaction.key}>
          <div className="redaction-bar">
            <span className="redaction-title">REDACTED BY THE LORD</span>
            <span className="redaction-rule">{redaction.rule}</span>
          </div>
        </div>
      )}
    </div>
  );
}
