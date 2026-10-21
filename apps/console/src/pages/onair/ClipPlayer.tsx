// The cut as a viewer sees it: the 9:16 short or the 16:9 broadcast trim played from the
// daemon's media route (it answers byte ranges, so the player seeks), and the cut's lines on
// the voice's real timing. A line jumps the player to it; a line the voice never said is
// marked. A clip without a recording plays its re-drawn draft and says so.
import type { ClipCandidate } from "@shadowrunr/contract";
import { useRef, useState } from "react";
import { readDataConfig } from "../../data";
import { Chip } from "../../ui/Chip";
import { PillGroup } from "../../ui/PillGroup";
import { Section } from "../../ui/Section";
import { ASPECT_LABEL, FOOTAGE_LABEL, aspects, clipMediaUrl, clock, cutFile, footageOf, speakerName, timedLines, type Aspect } from "./clipModel";
import styles from "./ClipPlayer.module.css";

/** The daemon this console talks to (`?daemon=`, `VITE_DAEMON_URL`, else the default). */
const daemonUrl = (): string => readDataConfig(window.location.search, import.meta.env).daemonUrl;

export function ClipPlayer({ c }: { c: ClipCandidate }) {
  const have = aspects(c);
  const [pick, setPick] = useState<Aspect>("9x16");
  const aspect = have.includes(pick) ? pick : have[0];
  const video = useRef<HTMLVideoElement>(null);
  const file = aspect ? cutFile(c, aspect) : undefined;
  const footage = FOOTAGE_LABEL[footageOf(c)];
  const lines = timedLines(c);
  const seek = (at: number) => {
    if (video.current) video.current.currentTime = at;
  };
  return (
    <Section title="Watch" right={footage}>
      <div className={styles.watch} data-role="onair-clip-player">
        <div className={styles.screen}>
          {have.length > 1 && (
            <PillGroup label="Aspect" value={aspect ?? "9x16"} onChange={setPick} options={have.map((a) => ({ id: a, label: ASPECT_LABEL[a] }))} />
          )}
          {file && aspect ? (
            <video
              key={file}
              ref={video}
              className={styles.video}
              data-aspect={aspect}
              data-role="onair-clip-video"
              src={clipMediaUrl(daemonUrl(), c.id, file)}
              controls
              preload="metadata"
              playsInline
            />
          ) : (
            <p className={styles.empty}>{c.render?.fallback_reason ? `No video yet: ${c.render.fallback_reason}` : "No video yet: render the clip."}</p>
          )}
        </div>
        <ol className={styles.lines} data-role="onair-clip-lines" aria-label={c.window.voice_timed ? "Lines on the voice's real timing" : "Lines on the estimated spoken clock"}>
          {lines.map((l, i) => (
            <li key={i} data-unheard={l.unheard || undefined}>
              <button type="button" className={styles.line} onClick={() => seek(l.at)} disabled={!file} title={`play from ${l.at.toFixed(1)} s`}>
                <small>{clock(l.streamMs)}</small>
                <b>{speakerName(l.speaker)}</b>
                <span>{l.text}</span>
                {l.unheard && <Chip tone="muted">never heard</Chip>}
              </button>
            </li>
          ))}
        </ol>
      </div>
      <p className={styles.note}>
        {c.window.voice_timed ? "Times are the voice's real playback, on the stream clock." : "Times are the caption model's estimate: this run has no voice log."}
        {c.recording ? ` Speech starts about ${c.recording.av_offset_ms} ms after each line's time (measured; the cut's pre-roll absorbs it).` : ""}
      </p>
    </Section>
  );
}
