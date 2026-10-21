// The owner's acts on one clip, as the daemon allows them in `acts` (the page never re-decides
// the rule). Approving is a signature, never a default, and an unmeasured check needs a
// recorded override reason. Publishing is a dry run that writes the request each platform would get;
// nothing is sent. Rendering records the stage to video when the daemon has the tools.
import type { ClipAct, ClipCandidate, ClipDryRun } from "@shadowrunr/contract";
import { useState, type FormEvent } from "react";
import { useMutation, viewOfError, type DataError } from "../../data";
import { Chip } from "../../ui/Chip";
import { Section } from "../../ui/Section";
import { blockText } from "./clipModel";
import styles from "./Clips.module.css";
import { Button } from "../../ui/Button";

/** `clips.render` records a video frame by frame: minutes, not seconds. */
const RENDER_TIMEOUT_MS = 15 * 60_000;

const explain = (e: DataError) => `${viewOfError(e).title}: ${e.message}`;

/** The daemon's block on `act`, `null` when allowed; a reply without `acts` allows nothing. */
const blockOf = (c: ClipCandidate, act: ClipAct): string | null => {
  const s = c.acts?.[act];
  return s === undefined ? "The daemon did not say what this clip allows." : s.blocked ? blockText(s.blocked) : null;
};

export function ClipActions({ c }: { c: ClipCandidate }) {
  return (
    <Section title="Owner acts" right="recorded on the clip">
      <Approve c={c} />
      <Reject c={c} />
      <div className={styles.actions}>
        <RenderButton c={c} />
        <Publish c={c} />
      </div>
      {(c.dry_runs?.length ?? 0) > 0 && <DryRuns rows={c.dry_runs ?? []} title="Earlier dry runs" />}
    </Section>
  );
}

function Approve({ c }: { c: ClipCandidate }) {
  const approve = useMutation("clips.approve");
  const [signature, setSignature] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | undefined>();
  const block = blockOf(c, "approve");
  const unmeasured = c.acts?.approve.override_needed ?? [];
  const needsReason = unmeasured.length > 0;
  const ready = block === null && signature.trim() !== "" && (!needsReason || reason.trim() !== "");

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!ready) return;
    setError(undefined);
    const r = await approve.run({ id: c.id, signature: signature.trim(), ...(needsReason ? { override_reason: reason.trim() } : {}) });
    if (!r.ok) setError(explain(r.error));
  }

  return (
    <form className={styles.form} onSubmit={(e) => void onSubmit(e)} data-role="onair-clip-approve">
      <label className={styles.field}>
        <span>Owner signature</span>
        <input type="text" value={signature} maxLength={80} disabled={block !== null} onChange={(e) => setSignature(e.target.value)} placeholder="your name" />
      </label>
      {needsReason && block === null && (
        <label className={`${styles.field} ${styles.grow}`} data-role="onair-clip-override">
          <span>Override reason ({unmeasured.join(", ")} unmeasured)</span>
          <input type="text" value={reason} maxLength={240} onChange={(e) => setReason(e.target.value)} placeholder="why approving without this check is sound" />
        </label>
      )}
      <Button type="submit" variant="primary" disabled={!ready || approve.pending}>
        {approve.pending ? "Signing…" : "Approve for upload"}
      </Button>
      {block !== null && <p className={styles.dim}>{block}</p>}
      {block === null && <p className={styles.dim}>Signing attests the cut keeps its context and the captions claim no more than it shows.</p>}
      {error && (
        <p className={styles.formError} role="alert">
          {error}
        </p>
      )}
    </form>
  );
}

function Reject({ c }: { c: ClipCandidate }) {
  const reject = useMutation("clips.reject");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | undefined>();
  if (blockOf(c, "reject") !== null) return null;

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!reason.trim()) return;
    setError(undefined);
    const r = await reject.run({ id: c.id, reason: reason.trim() });
    if (!r.ok) setError(explain(r.error));
  }

  return (
    <form className={styles.form} onSubmit={(e) => void onSubmit(e)} data-role="onair-clip-reject">
      <label className={`${styles.field} ${styles.grow}`}>
        <span>Reject with a reason</span>
        <input type="text" value={reason} maxLength={240} onChange={(e) => setReason(e.target.value)} placeholder="what is wrong with this cut" />
      </label>
      <Button type="submit" variant="secondary" disabled={!reason.trim() || reject.pending}>
        {reject.pending ? "Rejecting…" : "Reject"}
      </Button>
      {error && (
        <p className={styles.formError} role="alert">
          {error}
        </p>
      )}
    </form>
  );
}

function RenderButton({ c }: { c: ClipCandidate }) {
  const render = useMutation("clips.render", { timeoutMs: RENDER_TIMEOUT_MS });
  const [error, setError] = useState<string | undefined>();
  // The layer owns the call: this clip's render keeps showing while the operator visits others.
  const rendering = render.pending || render.running.some((f) => (f.params as { id?: string }).id === c.id);
  // This clip's render failure that landed while the button was not mounted shows until the next render.
  const missed = render.settled?.params.id === c.id ? render.settled : undefined;
  const shownError = error ?? (missed?.error && explain(missed.error));
  const block = blockOf(c, "render");
  return (
    <span className={styles.rowAction}>
      <Button
        type="button"
        variant="secondary"
        disabled={block !== null || rendering}
        title={block ?? "record the stage to a vertical MP4, or a poster frame when the tools are missing"}
        onClick={() => {
          missed?.ack();
          void render.run({ id: c.id }).then((r) => setError(r.ok ? undefined : explain(r.error)));
        }}
      >
        {rendering ? "Rendering…" : "Render draft"}
      </Button>
      {shownError && <small role="alert">{shownError}</small>}
    </span>
  );
}

function Publish({ c }: { c: ClipCandidate }) {
  const publish = useMutation("clips.publish");
  const [result, setResult] = useState<ClipDryRun[] | null>(null);
  const [error, setError] = useState<string | undefined>();
  const block = blockOf(c, "publish");
  return (
    <span className={styles.rowAction}>
      <Button
        type="button"
        variant="secondary"
        disabled={block !== null || publish.pending}
        data-role="onair-clip-publish"
        title={block ?? "write the exact request each platform would get; nothing is sent"}
        onClick={() =>
          void publish.run({ id: c.id }).then((r) => {
            if (r.ok) {
              setResult(r.value.requests);
              setError(undefined);
            } else setError(explain(r.error));
          })
        }
      >
        {publish.pending ? "Writing…" : "Publish (dry run)"}
      </Button>
      {error && <small role="alert">{error}</small>}
      {result && <DryRuns rows={result} title="DRY RUN — nothing is sent" />}
    </span>
  );
}

function DryRuns({ rows, title }: { rows: readonly ClipDryRun[]; title: string }) {
  return (
    <div className={styles.dry} data-role="onair-clip-dry-runs">
      <b>{title}</b>
      <ul className={styles.plain}>
        {rows.map((d, i) => (
          <li key={i}>
            <Chip tone={d.applicable ? "ok" : "muted"}>{d.platform}</Chip> {d.applicable ? "request written" : "not applicable"} · <small>{d.file}</small>
            {d.budget_left !== undefined && <small> · {d.budget_left} left today</small>}
            <span className={styles.dim}> {d.note}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
