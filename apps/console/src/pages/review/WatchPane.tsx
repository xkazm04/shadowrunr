// The player's watch pane: the session's own recording (`clips.recording`, played from the
// daemon's media route) seeked to the line on screen, so a line is judged as the viewer saw
// and heard it. Off until asked for: the recording is hundreds of megabytes.
import { useEffect, useRef, useState } from "react";
import type { ClipRunRecording } from "@shadowrunr/contract";
import { readDataConfig, useRpc } from "../../data";
import { Switch } from "../../ui/Switch";
import { fmtClock } from "./names";
import styles from "./WatchPane.module.css";
import { fileSeconds, lineStartMs, mediaUrl } from "./watch";

export function WatchPane(props: { sessionId: string; line: number; t: number }) {
  const [on, setOn] = useState(false);
  return (
    <section className={styles.pane} data-role="review-watch" aria-label="Watch the recording">
      <Switch checked={on} onChange={setOn} label="Watch this line in the recording" />
      {on && <Recording {...props} />}
    </section>
  );
}

function Recording({ sessionId, line, t }: { sessionId: string; line: number; t: number }) {
  const q = useRpc("clips.recording", { session_id: sessionId });
  if (q.error?.code === "not_found") return <p className={styles.note}>This session has no recording. Record one with <code>shadowrunr stream sandbox --record</code>.</p>;
  if (q.data === undefined) return <p className={styles.note}>{q.errorView ? `${q.errorView.title}: ${q.error?.message ?? ""}` : "Loading the recording…"}</p>;
  return <Seeked rec={q.data} line={line} t={t} />;
}

function Seeked({ rec, line, t }: { rec: ClipRunRecording; line: number; t: number }) {
  const video = useRef<HTMLVideoElement>(null);
  const start = lineStartMs(rec, line, t * 1000);
  const at = fileSeconds(rec, start.ms);
  useEffect(() => {
    const v = video.current;
    if (!v) return;
    // Before the metadata arrives a seek is dropped; seek again once it does.
    const seek = () => {
      v.currentTime = at;
    };
    seek();
    v.addEventListener("loadedmetadata", seek);
    return () => v.removeEventListener("loadedmetadata", seek);
  }, [at]);
  const daemon = readDataConfig(window.location.search, import.meta.env).daemonUrl;
  return (
    <>
      <video ref={video} className={styles.video} src={mediaUrl(daemon, rec)} controls preload="metadata" playsInline data-role="review-watch-video" data-at={at.toFixed(3)} />
      <p className={styles.note} data-role="review-watch-where">
        Line {line} {start.heard ? "starts" : "was never heard; its transcript time is"} at {fmtClock(start.ms / 1000)} on the stream, {at.toFixed(1)} s into the recording.
        {start.heard ? ` Speech follows about ${rec.av_offset_ms} ms later.` : ""}
      </p>
    </>
  );
}
