// characters/<id>/package/voice: who speaks for the character, and how. The package's [voice] is
// the default; a local override on this machine (voice.json, gitignored) wins per field and takes
// effect at the next stream start. Each provider's readiness with a key check (the key lives in
// the daemon's environment and never reaches the console), a preview through the output gate,
// and the emotion bench (VoiceLab.tsx).
import { useState } from "react";
import type { CharacterDetail, ProviderStatus, VoiceChoice, VoiceProvider, VoiceSettings } from "@shadowrunr/contract";
import { useMutation, useRpc } from "../../data";
import { QueryView } from "../../kit";
import { timeOf } from "../../kit/freshness";
import { Button } from "../../ui/Button";
import { Chip } from "../../ui/Chip";
import { PillGroup } from "../../ui/PillGroup";
import { Head } from "../../ui/Section";
import { StatePanel } from "../../ui/States";
import styles from "./Layer2.module.css";
import { FX_NAME, FX_ORDER, KEY_NAME, PROVIDER_NAME, PROVIDER_ORDER } from "./names";
import lab from "./ToneLab.module.css";
import vs from "./Voice.module.css";
import { VoiceBenchPanel, VoicePreviewPanel } from "./VoiceLab";
import { buildChoice, fieldsOf, SPEED_RANGE, voiceError, type VoiceFields } from "./voiceModel";

export function VoiceTab({ d }: { d: CharacterDetail }) {
  const q = useRpc("voice.settings.get", { character: d.id });
  // The daemon's own "unavailable" (its voice service cannot answer) is not an offline daemon.
  const refused = q.data === undefined && q.error !== undefined ? voiceError(q.error) : undefined;
  return (
    <div data-role="characters-voice-tab">
      <Head
        kicker="Characters · the package"
        title="Voice"
        lede={`Who speaks for ${d.name}: the package sets the default, and an override on this machine wins. Preview a line through the output gate, and render the emotion bench.`}
      />
      {refused && refused.state !== "offline" ? (
        <StatePanel state={refused.state} title={refused.title} hint={refused.hint} detail={`${refused.code}: ${refused.message}`} action={{ label: "Retry", onClick: q.reload }} />
      ) : (
        <QueryView query={q}>
          {(s) => (
            <div className={lab.grid}>
              <div className={lab.col}>
                <VoiceForm s={s} />
                <VoiceStatus s={s} />
              </div>
              <div className={lab.col}>
                <VoicePreviewPanel character={d.id} />
                <VoiceBenchPanel character={d.id} />
              </div>
            </div>
          )}
        </QueryView>
      )}
    </div>
  );
}

function Field(props: { label: string; value: string; placeholder: string; onChange: (v: string) => void; role: string; numeric?: boolean }) {
  return (
    <label className={vs.field}>
      <span className={lab.label}>{props.label}</span>
      <input
        className={vs.input}
        type="text"
        inputMode={props.numeric ? "decimal" : undefined}
        value={props.value}
        placeholder={props.placeholder}
        maxLength={120}
        spellCheck={false}
        onChange={(e) => props.onChange(e.target.value)}
        data-role={props.role}
      />
    </label>
  );
}

const pkgText = (v: string | number | undefined): string => (v === undefined ? "the daemon's default" : `package: ${v}`);

/**
 * The override form: the package's value is each field's placeholder, the override its value. It
 * holds the owner's draft; a save or reset loads what the daemon stored.
 */
function VoiceForm({ s }: { s: VoiceSettings }) {
  const set = useMutation("voice.settings.set");
  const [provider, setProvider] = useState<VoiceProvider>(s.override?.provider ?? s.effective.provider);
  const [f, setF] = useState<VoiceFields>(fieldsOf(s.override));
  const [outcome, setOutcome] = useState<{ ok: boolean; text: string } | undefined>(undefined);
  const put = (patch: Partial<VoiceFields>) => {
    setF((x) => ({ ...x, ...patch }));
    setOutcome(undefined);
  };
  const built = buildChoice(provider, f);
  const p = s.package;
  const write = async (choice: VoiceChoice | null, done: string) => {
    setOutcome(undefined);
    const r = await set.run({ character: s.character, choice });
    if (r.ok) {
      // The form shows what was written: the override as stored, or none after a reset.
      setF(fieldsOf(r.value.override));
      setProvider(r.value.override?.provider ?? r.value.effective.provider);
      setOutcome({ ok: true, text: done });
    } else {
      const v = voiceError(r.error);
      setOutcome({ ok: false, text: `${v.title}: ${r.error.message}` });
    }
  };
  const [klo, khi] = SPEED_RANGE.kokoro;
  const [elo, ehi] = SPEED_RANGE.elevenlabs;
  return (
    <section className={lab.panel} data-role="voice-choice" aria-labelledby="voice-choice">
      <h2 className={styles.secHead} id="voice-choice">
        Provider and voice
      </h2>
      <p className={vs.now} data-role="voice-effective">
        <span className={lab.label}>Now</span> {s.label}
        <Chip tone={s.override ? "signal" : "muted"}>{s.override ? "local override" : "package default"}</Chip>
      </p>
      <div className={styles.toolsRow}>
        <PillGroup label="Provider" options={PROVIDER_ORDER.map((id) => ({ id, label: PROVIDER_NAME[id] }))} value={provider} onChange={(v) => { setProvider(v); setOutcome(undefined); }} />
      </div>
      {provider === "kokoro" ? (
        <div className={vs.fields} data-role="voice-kokoro">
          <Field label="Voice" value={f.voice} placeholder={pkgText(p.kokoro?.voice)} onChange={(voice) => put({ voice })} role="voice-kokoro-voice" />
          <Field label="Speaker id" value={f.sid} placeholder={pkgText(p.kokoro?.sid)} onChange={(sid) => put({ sid })} role="voice-kokoro-sid" numeric />
          <label className={vs.field}>
            <span className={lab.label}>Effect</span>
            <select className={vs.input} value={f.fx} onChange={(e) => put({ fx: e.target.value as VoiceFields["fx"] })} data-role="voice-kokoro-fx">
              <option value="">{p.kokoro?.fx === undefined ? "The daemon's default" : `Package: ${FX_NAME[p.kokoro.fx]}`}</option>
              {FX_ORDER.map((x) => (
                <option key={x} value={x}>
                  {FX_NAME[x]}
                </option>
              ))}
            </select>
          </label>
          <Field label={`Speed (${klo}–${khi})`} value={f.kokoroSpeed} placeholder={pkgText(p.kokoro?.speed)} onChange={(kokoroSpeed) => put({ kokoroSpeed })} role="voice-kokoro-speed" numeric />
        </div>
      ) : (
        <div className={vs.fields} data-role="voice-elevenlabs">
          <Field label="Voice id" value={f.voiceId} placeholder={pkgText(p.elevenlabs?.voice_id)} onChange={(voiceId) => put({ voiceId })} role="voice-eleven-voice" />
          <Field label="Model" value={f.model} placeholder={pkgText(p.elevenlabs?.model)} onChange={(model) => put({ model })} role="voice-eleven-model" />
          <Field label={`Speed (${elo}–${ehi})`} value={f.elevenSpeed} placeholder={pkgText(p.elevenlabs?.speed)} onChange={(elevenSpeed) => put({ elevenSpeed })} role="voice-eleven-speed" numeric />
        </div>
      )}
      <div className={lab.actions}>
        <Button variant="ink" disabled={set.pending || !built.ok} onClick={() => built.ok && void write(built.choice, "Saved")} data-role="voice-save">
          {set.pending ? "Saving…" : "Save override"}
        </Button>
        <Button disabled={set.pending || s.override === undefined} onClick={() => void write(null, "Back to the package default")} data-role="voice-reset">
          Reset to package default
        </Button>
        {!built.ok && <span className={lab.err}>{built.error}</span>}
        {outcome && <span className={outcome.ok ? lab.ok : lab.err} role="status">{outcome.text}</span>}
      </div>
      <p className={styles.note}>Takes effect at the next stream start. The override stays on this machine; the package is not changed.</p>
    </section>
  );
}

/** Each provider's readiness. A newer key check (voice.verify) wins over the settings' row. */
function VoiceStatus({ s }: { s: VoiceSettings }) {
  const verify = useMutation("voice.verify");
  const [checked, setChecked] = useState<Partial<Record<VoiceProvider, ProviderStatus>>>({});
  const [failed, setFailed] = useState<Partial<Record<VoiceProvider, string>>>({});
  const check = async (provider: VoiceProvider) => {
    setFailed((x) => ({ ...x, [provider]: undefined }));
    const r = await verify.run({ provider });
    if (r.ok) setChecked((x) => ({ ...x, [provider]: r.value }));
    else setFailed((x) => ({ ...x, [provider]: `${voiceError(r.error).title}: ${r.error.message}` }));
  };
  const rows = s.providers.map((row) => {
    const mine = checked[row.provider];
    return mine && (row.checked_ms === undefined || (mine.checked_ms ?? 0) >= row.checked_ms) ? mine : row;
  });
  return (
    <section className={lab.panel} data-role="voice-status" aria-labelledby="voice-status">
      <h2 className={styles.secHead} id="voice-status">
        Provider status
      </h2>
      {rows.length === 0 ? (
        <StatePanel state="empty" title="No provider reported" hint="This daemon lists no voice provider." />
      ) : (
        <ul className={lab.list}>
          {rows.map((r) => (
            <li key={r.provider} className={lab.item} data-role="voice-status-row" data-provider={r.provider} data-ready={r.ready}>
              <div className={lab.meta}>
                <b className={vs.who}>{PROVIDER_NAME[r.provider]}</b>
                <Chip tone={r.ready ? "ok" : "fail"}>{r.ready ? "Ready" : "Not ready"}</Chip>
                <Chip tone={r.key === "rejected" || r.key === "missing" ? "caution" : "neutral"}>{KEY_NAME[r.key]}</Chip>
                {r.checked_ms !== undefined && <span className={styles.dim}>checked {timeOf(r.checked_ms)}</span>}
                {r.key !== "not_needed" && (
                  <Button size="compact" disabled={verify.pending} onClick={() => void check(r.provider)} data-role="voice-verify">
                    Check key
                  </Button>
                )}
              </div>
              {r.reason !== undefined && <p className={lab.noteText}>{r.reason}</p>}
              {failed[r.provider] !== undefined && <span className={lab.err}>{failed[r.provider]}</span>}
            </li>
          ))}
        </ul>
      )}
      <p className={styles.note}>
        Set <code>ELEVENLABS_API_KEY</code> in the daemon&apos;s environment. The key never reaches the console, and there is no field for it here.
      </p>
    </section>
  );
}
