// The owner's directions, one editor shared by the pages that write them: a plain text area
// saved whole (DirectionsEditor), and its two ready uses. A segment's directions
// (segments.directions.get / .set, `segments/<SEG-ID>.md`, read into that segment's turn while it
// is live) on the Segments page and from a Review line; the behaviour directions
// (tone.directions.get / .set, `directions.md`, read on every stream) in the Tone Lab and from a
// Review line. The draft is the file until it is edited; Save sends it, Discard drops it. An
// `append` request adds its text at the end of the draft once per key, only after the file has
// loaded, so a save never drops what the file already held. Nothing is written until Save.
import { useState } from "react";
import type { SegmentId } from "@shadowrunr/contract";
import { useMutation, useRpc, viewOfError, type DataError, type Query, type Result } from "../data";
import { Button } from "../ui/Button";
import { StatePanel } from "../ui/States";
import dir from "./Directions.module.css";
import { QueryView } from "./QueryView";

/** Text to append to a directions draft; a new `key` appends again. */
export interface Append {
  readonly key: number;
  readonly text: string;
}

let seq = 0;
/** A fresh append request for `text`. */
export const appendOf = (text: string): Append => ({ key: ++seq, text });

/** The longest directions file the daemon writes, in characters. */
export const MAX_DIRECTIONS_CHARS = 20000;

export interface DirectionsEditorProps<T extends { readonly text: string }> {
  /** The file as it stands. */
  readonly query: Query<T>;
  /** Writes the whole text; the editor reloads `query` after a save. */
  readonly save: (text: string) => Promise<Result<T, DataError>>;
  readonly saving: boolean;
  /** The text area's accessible name. */
  readonly label: string;
  readonly saveLabel: string;
  /** `data-role` prefix: `<role>-text` and `<role>-save`. */
  readonly role: string;
  /** Text to add at the end of the draft; a new `key` adds again. */
  readonly append?: Append | undefined;
  readonly onSaved?: (saved: T) => void;
}

export function DirectionsEditor<T extends { readonly text: string }>(props: DirectionsEditorProps<T>) {
  const { query: q } = props;
  const [draft, setDraft] = useState<string | undefined>(undefined);
  const [failure, setFailure] = useState<DataError | undefined>(undefined);
  const [saved, setSaved] = useState(false);
  const text = draft ?? q.data?.text ?? "";
  const dirty = draft !== undefined && draft !== q.data?.text;

  // An append lands in the draft once per key, once the file is in (state adjusted in render).
  const [appended, setAppended] = useState<number | undefined>(undefined);
  if (props.append && props.append.key !== appended && q.data !== undefined) {
    setAppended(props.append.key);
    const base = text.replace(/\s+$/, "");
    setDraft(`${base}${base === "" ? "" : "\n"}${props.append.text}\n`);
    setSaved(false);
  }

  const run = async () => {
    setFailure(undefined);
    const r = await props.save(text);
    if (!r.ok) return setFailure(r.error);
    setDraft(undefined);
    setSaved(true);
    props.onSaved?.(r.value);
    q.reload();
  };

  return (
    <QueryView query={q}>
      {() => (
        <>
          <textarea
            className={dir.field}
            aria-label={props.label}
            value={text}
            maxLength={MAX_DIRECTIONS_CHARS}
            onChange={(e) => {
              setDraft(e.target.value);
              setSaved(false);
            }}
            data-role={`${props.role}-text`}
          />
          <div className={dir.actions}>
            <Button variant="ink" disabled={props.saving || !dirty} onClick={() => void run()} data-role={`${props.role}-save`}>
              {props.saving ? "Saving…" : props.saveLabel}
            </Button>
            {dirty && (
              <Button size="compact" onClick={() => setDraft(undefined)}>
                Discard changes
              </Button>
            )}
            {saved && !dirty && <span className={dir.ok}>Saved</span>}
          </div>
          {failure && <StatePanel state={viewOfError(failure).state} title={`Not saved · ${viewOfError(failure).title}`} hint={viewOfError(failure).hint} detail={`${failure.code}: ${failure.message}`} />}
        </>
      )}
    </QueryView>
  );
}

// ── The two uses ─────────────────────────────────────────────────────

const SEGMENT_IDS: readonly SegmentId[] = ["SEG-01", "SEG-02A", "SEG-02B-I", "SEG-02B-II", "SEG-03"];

/** Whether `s` is a segment id the contract knows. */
export const isSegmentId = (s: string | undefined): s is SegmentId => SEGMENT_IDS.includes(s as SegmentId);

/** The package-relative file a segment's directions live in. */
export const segmentDirectionsFile = (segment: SegmentId): string => `segments/${segment}.md`;

/** `> L123 "line text"`: a Review line quoted into a directions draft, on one line. */
export const quoteLine = (line: number, text: string): string => `> L${line} "${text.replace(/\s+/g, " ").trim()}"`;

interface EditorProps {
  readonly character: string;
  /** `data-role` prefix of the text area and the save button. */
  readonly role: string;
  readonly append?: Append | undefined;
  readonly onSaved?: (file: string) => void;
}

export function SegmentDirectionsEditor(props: EditorProps & { readonly segment: SegmentId }) {
  const { character, segment } = props;
  const q = useRpc("segments.directions.get", { character, segment_id: segment });
  const save = useMutation("segments.directions.set");
  return (
    <DirectionsEditor
      query={q}
      save={(text) => save.run({ character, segment_id: segment, text })}
      saving={save.pending}
      label="Directions for this segment"
      saveLabel="Save segment directions"
      role={props.role}
      append={props.append}
      onSaved={() => props.onSaved?.(segmentDirectionsFile(segment))}
    />
  );
}

export function BehaviourDirectionsEditor(props: EditorProps) {
  const { character } = props;
  const q = useRpc("tone.directions.get", { character });
  const save = useMutation("tone.directions.set");
  return (
    <DirectionsEditor
      query={q}
      save={(text) => save.run({ character, text })}
      saving={save.pending}
      label="Directions"
      saveLabel="Save directions"
      role={props.role}
      append={props.append}
      onSaved={(d) => props.onSaved?.(d.file)}
    />
  );
}
