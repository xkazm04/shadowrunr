// The Tone Lab: the owner directs the character's voice. One tone at a time: its line bank
// (approve, retire, edit as new), variants from the core model (tone.variants: pick a
// situation, optionally a line and a note, approve a candidate as written or edited), the
// directions file, and the inbox of Review feedback. A Review line arrives by route
// (characters/<id>/package/tone-lab/<session>/<line>) and lands in the variants panel. The
// daemon writes lines/<tone>.toml and directions.md in the package; the owner commits them.
import { useState } from "react";
import type { CharacterDetail } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { Head } from "../../ui/Section";
import { PillGroup } from "../../ui/PillGroup";
import { StatePanel } from "../../ui/States";
import { toneLabel } from "./model";
import type { ReviewRef } from "./routes";
import styles from "./Layer2.module.css";
import lab from "./ToneLab.module.css";
import { ToneBank } from "./ToneBank";
import { ToneDirections, ToneInbox } from "./ToneSide";
import { reviewRef } from "./toneNames";
import { nextKey, type Append, type LabProps, type Seed } from "./toneTypes";
import { ToneVariants } from "./ToneVariants";

export function ToneLab({ d, from }: { d: CharacterDetail; from: ReviewRef | undefined }) {
  const tones = d.tones.map((t) => t.id);
  const [tone, setTone] = useState(tones.includes(d.active_tone) ? d.active_tone : (tones[0] ?? d.default_tone));
  const [seed, setSeed] = useState<Seed | undefined>(undefined);
  const [append, setAppend] = useState<Append | undefined>(undefined);
  const [changed, setChanged] = useState<readonly string[]>([]);
  const wrote = (file: string) => setChanged((xs) => (xs.includes(file) ? xs : [...xs, file]));
  const sent = useRpc("tone.review_line", from ? { character: d.id, session_id: from.session, line: from.line } : null);
  const item = sent.data;

  // A Review line seeds the variants panel once, when it arrives (state adjusted in render, not
  // in an effect).
  const [applied, setApplied] = useState<typeof item>(undefined);
  if (item && from && item !== applied) {
    setApplied(item);
    if (tones.includes(item.tone)) setTone(item.tone);
    setSeed({
      key: nextKey(),
      line: item.text,
      ...(item.note === undefined ? {} : { note: item.note }),
      ...(item.situation === undefined ? {} : { situation: item.situation }),
      source: reviewRef(item.session_id, item.line),
    });
  }

  const props: LabProps = { character: d.id, tone, onWrote: wrote };
  return (
    <div data-role="characters-tone-lab">
      <Head
        kicker="Characters · the package"
        title="Tone Lab"
        lede="Direct the voice: approve the lines the model is shown, draft new ones for a situation, and keep the do and don't list. Review feedback lands in the inbox."
      />
      <p className={lab.commit} data-role="tone-lab-commit">
        The Tone Lab writes into this package in the repo (<code>lines/&lt;tone&gt;.toml</code> and <code>directions.md</code>). Changed files stay uncommitted until you commit them.
        {changed.length > 0 && (
          <>
            {" "}
            Changed here: <b>{changed.join(", ")}</b>.
          </>
        )}
      </p>
      {tones.length === 0 ? (
        <StatePanel state="empty" title="No tones yet" hint="Add tones/<id>.md to the package: each tone has its own line bank." />
      ) : (
        <>
          <div className={styles.toolsRow}>
            <PillGroup label="Tone" options={tones.map((t) => ({ id: t, label: toneLabel(t) }))} value={tone} onChange={setTone} />
          </div>
          {from && sent.data === undefined && (sent.errorView ? (
            <StatePanel state={sent.errorView.state} title={`The Review line did not arrive · ${sent.errorView.title}`} detail={`${sent.errorView.code}: ${sent.errorView.message}`} action={{ label: "Retry", onClick: sent.reload }} />
          ) : (
            <StatePanel state="loading" title="Fetching the Review line" />
          ))}
          <div className={lab.grid}>
            <div className={lab.col}>
              <ToneVariants key={`${tone}:${seed?.key ?? 0}`} {...props} seed={seed} />
              <ToneBank {...props} onImprove={setSeed} />
            </div>
            <div className={lab.col}>
              <ToneInbox {...props} tones={tones} onImprove={setSeed} onSuggest={(text) => setAppend({ key: nextKey(), text })} />
              <ToneDirections {...props} append={append} />
            </div>
          </div>
        </>
      )}
    </div>
  );
}
