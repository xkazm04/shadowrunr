// The Voice tab's lab: a preview of one line (voice.preview: the brain's output gate first, then
// the character's effective voice, 400 chars at most) with a delivery to try, and the emotion
// bench (voice.bench: the package's voice/bench.toml rendered line by line with the same choice).
// Audio plays here through WebAudio (voiceAudio.ts); nothing is written anywhere.
import { useRef, useState } from "react";
import type { Delivery, Emotion, Pace, VoiceBench, VoicePreview } from "@shadowrunr/contract";
import { useMutation, type DataError } from "../../data";
import { int } from "../../kit/events";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { StatePanel } from "../../ui/States";
import styles from "./Layer2.module.css";
import { deliveryLabel, EMOTION_NAME, EMOTION_ORDER, PACE_NAME, PACE_ORDER, PROVIDER_NAME } from "./names";
import lab from "./ToneLab.module.css";
import vs from "./Voice.module.css";
import { playPcm, stopPcm } from "./voiceAudio";
import { PREVIEW_MAX, secs, voiceError } from "./voiceModel";

/** A cloud provider plus the gate and the render; the bench renders every line in one call. */
const PREVIEW_TIMEOUT_MS = 60_000;
const BENCH_TIMEOUT_MS = 180_000;

const play = (pcm: string, rate: number, onError: (e: string) => void) => playPcm(pcm, rate).catch((e: unknown) => onError(e instanceof Error ? e.message : String(e)));

function Failure({ error, what }: { error: DataError; what: string }) {
  if (error.code === "invalid_params") {
    return <StatePanel state="invalid" title="The output gate refused this line" detail={error.message} />;
  }
  const v = voiceError(error);
  return <StatePanel state={v.state} title={`${what} · ${v.title}`} hint={v.hint} detail={`${v.code}: ${v.message}`} />;
}

export function VoicePreviewPanel({ character }: { character: string }) {
  const preview = useMutation("voice.preview", { timeoutMs: PREVIEW_TIMEOUT_MS });
  const [text, setText] = useState("");
  const [emotion, setEmotion] = useState<Emotion>("neutral");
  const [intensity, setIntensity] = useState(0.6);
  const [pace, setPace] = useState<Pace>("normal");
  const [out, setOut] = useState<VoicePreview | undefined>(undefined);
  const [failure, setFailure] = useState<DataError | undefined>(undefined);
  const [audioError, setAudioError] = useState<string | undefined>(undefined);

  const speak = async () => {
    setFailure(undefined);
    setAudioError(undefined);
    const delivery: Delivery = { emotion, intensity, pace };
    const r = await preview.run({ character, text: text.trim(), delivery });
    if (!r.ok) {
      setOut(undefined);
      setFailure(r.error);
      return;
    }
    setOut(r.value);
    await play(r.value.pcm, r.value.sample_rate, setAudioError);
  };

  return (
    <section className={lab.panel} data-role="voice-preview" aria-labelledby="voice-preview">
      <h2 className={styles.secHead} id="voice-preview">
        Preview
      </h2>
      <label className={lab.label} htmlFor="voice-preview-text">
        A line to speak · {int(text.length)} of {PREVIEW_MAX}
      </label>
      <textarea
        id="voice-preview-text"
        className={lab.field}
        value={text}
        maxLength={PREVIEW_MAX}
        placeholder="Kneel, mortals. The stream begins."
        onChange={(e) => setText(e.target.value)}
        data-role="voice-preview-text"
      />
      <div className={styles.toolsRow}>
        <label className={styles.fld}>
          Emotion
          <select value={emotion} onChange={(e) => setEmotion(e.target.value as Emotion)} data-role="voice-preview-emotion">
            {EMOTION_ORDER.map((x) => (
              <option key={x} value={x}>
                {EMOTION_NAME[x]}
              </option>
            ))}
          </select>
        </label>
        <label className={styles.fld}>
          Intensity {Math.round(intensity * 100)}%
          <input className={vs.range} type="range" min={0} max={1} step={0.05} value={intensity} onChange={(e) => setIntensity(Number(e.target.value))} data-role="voice-preview-intensity" />
        </label>
        <label className={styles.fld}>
          Pace
          <select value={pace} onChange={(e) => setPace(e.target.value as Pace)} data-role="voice-preview-pace">
            {PACE_ORDER.map((x) => (
              <option key={x} value={x}>
                {PACE_NAME[x]}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className={lab.actions}>
        <Button variant="ink" disabled={preview.pending || text.trim() === ""} onClick={() => void speak()} data-role="voice-preview-speak">
          {preview.pending ? "Rendering…" : "Speak"}
        </Button>
        {out && (
          <Button size="compact" onClick={() => void play(out.pcm, out.sample_rate, setAudioError)} data-role="voice-preview-replay">
            Play again
          </Button>
        )}
        <span className={styles.dim}>The line passes the output gate first, as on stream.</span>
      </div>
      {failure && <Failure error={failure} what="No preview" />}
      {out && (
        <p className={lab.meta} data-role="voice-preview-result">
          <Chip tone="signal">{PROVIDER_NAME[out.provider]}</Chip>
          <span>{out.label}</span>
          <span className={styles.dim}>
            {secs(out.duration_ms)} · {int(out.chars)} chars · {out.visemes ? "mouth shapes" : "mouth by loudness"}
          </span>
        </p>
      )}
      {audioError !== undefined && <span className={lab.err}>{audioError}</span>}
    </section>
  );
}

export function VoiceBenchPanel({ character }: { character: string }) {
  const render = useMutation("voice.bench", { timeoutMs: BENCH_TIMEOUT_MS });
  const [bench, setBench] = useState<VoiceBench | undefined>(undefined);
  const [failure, setFailure] = useState<DataError | undefined>(undefined);
  const [playing, setPlaying] = useState<string | undefined>(undefined);
  const [audioError, setAudioError] = useState<string | undefined>(undefined);
  // Bumped by Stop and by each new play, so a running Play all ends at its next line.
  const run = useRef(0);

  const go = async () => {
    setFailure(undefined);
    const r = await render.run({ character });
    if (r.ok) setBench(r.value);
    else {
      setBench(undefined);
      setFailure(r.error);
    }
  };

  const playLines = async (ids: readonly string[]) => {
    if (!bench) return;
    const mine = ++run.current;
    setAudioError(undefined);
    for (const id of ids) {
      const l = bench.lines.find((x) => x.id === id);
      if (!l || l.error !== undefined || run.current !== mine) continue;
      setPlaying(id);
      await play(l.pcm, l.sample_rate, (e) => {
        setAudioError(e);
        run.current += 1;
      });
    }
    if (run.current === mine) setPlaying(undefined);
  };

  const stop = () => {
    run.current += 1;
    stopPcm();
    setPlaying(undefined);
  };

  const missing = failure?.code === "not_found" || (bench !== undefined && bench.lines.length === 0);
  const playable = bench?.lines.filter((l) => l.error === undefined).map((l) => l.id) ?? [];
  return (
    <section className={lab.panel} data-role="voice-bench" aria-labelledby="voice-bench">
      <h2 className={styles.secHead} id="voice-bench">
        Emotion bench
      </h2>
      <div className={lab.actions}>
        <Button variant="ink" disabled={render.pending} onClick={() => void go()} data-role="voice-bench-render">
          {render.pending ? "Rendering…" : bench ? "Render again" : "Render bench"}
        </Button>
        {playable.length > 0 && (
          <Button size="compact" onClick={() => void playLines(playable)} data-role="voice-bench-play-all">
            Play all
          </Button>
        )}
        {playing !== undefined && (
          <Button size="compact" onClick={stop} data-role="voice-bench-stop">
            Stop
          </Button>
        )}
        <span className={styles.dim}>Each line in the package&apos;s bench, spoken with its delivery by the voice in effect.</span>
      </div>
      {missing ? (
        <StatePanel state="empty" title="No emotion bench" hint="Add voice/bench.toml to the package: one [[line]] per test, with id, text, emotion, intensity and pace." />
      ) : (
        failure && <Failure error={failure} what="No bench" />
      )}
      {bench && bench.lines.length > 0 && (
        <>
          <p className={styles.note}>
            {PROVIDER_NAME[bench.provider]} · {bench.label}
          </p>
          <ol className={lab.list} data-role="voice-bench-lines">
            {bench.lines.map((l) => (
              <li key={l.id} className={lab.item} data-role="voice-bench-line" data-line-id={l.id} data-playing={playing === l.id ? "true" : undefined} data-failed={l.error !== undefined ? "true" : undefined}>
                <p className={lab.text}>{l.text}</p>
                <div className={lab.meta}>
                  <Chip tone="neutral">{deliveryLabel(l.delivery)}</Chip>
                  {l.error === undefined ? (
                    <>
                      <span className={styles.dim}>
                        {secs(l.duration_ms)} · {int(l.chars)} chars
                      </span>
                      <Button size="compact" onClick={() => void playLines([l.id])} aria-label={`Play ${l.id}`}>
                        {playing === l.id ? "Playing…" : "Play"}
                      </Button>
                    </>
                  ) : (
                    <span className={lab.err}>{l.error}</span>
                  )}
                </div>
              </li>
            ))}
          </ol>
        </>
      )}
      {audioError !== undefined && <span className={lab.err}>{audioError}</span>}
    </section>
  );
}
