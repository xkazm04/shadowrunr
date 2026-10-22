// The package's parameters (character.toml v2), read-only: who the character is in the engine's
// own text ([identity]), its voice provider (the voice itself is the Voice tab's) and face, its
// rank-ladder overrides, the lesson-tuned constants against the engine defaults, and the flavour
// it adds to each segment. Editing stays in the package file; this tab shows what will run.
import type { CharacterDetail, Rank, SegmentId, TuningRow } from "@shadowrunr/contract";
import type { ReactNode } from "react";
import { RouteLink } from "../../router";
import { buttonClass } from "../../ui/Button";
import { Head, Section } from "../../ui/Section";
import { EmptyState } from "../../ui/States";
import { faceLabel, PROVENANCE_NAME, PROVIDER_NAME, RANK_ORDER, rankLabel, SEGMENT_NAME, stateLabel, tuningLabel, tuningValue } from "./names";
import l2 from "./Layer2.module.css";
import { pkgRoute } from "./routes";
import styles from "./Params.module.css";

const KICKER = "Characters · the package";

function Kv({ rows }: { rows: readonly (readonly [string, ReactNode | undefined])[] }) {
  const shown = rows.filter((r): r is readonly [string, ReactNode] => r[1] !== undefined && r[1] !== "");
  return (
    <dl className={l2.kv}>
      {shown.map(([k, v]) => (
        <div key={k} className={l2.kvRow}>
          <dt>{k}</dt>
          <dd>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

const words = (ws: readonly string[] | undefined) => (ws && ws.length > 0 ? ws.join(" · ") : undefined);

function Identity({ d }: { d: CharacterDetail }) {
  const id = d.identity;
  if (!id) {
    return (
      <Section title="Identity">
        <EmptyState title="No identity table" hint={`The engine uses a neutral identity built from the name "${d.name}": viewers are viewers, the patron is the producer.`} />
      </Section>
    );
  }
  return (
    <Section title="Identity">
      <div data-role="characters-identity">
        <Kv
          rows={[
            ["Name", id.name],
            ["Title", id.title],
            ["Called by", words(id.address_words)],
            ["Reply labels", words(id.reply_prefixes)],
            ["Patron", `${id.patron.name} (${id.patron.title})`],
            ["Viewers", `${id.follower.one} / ${id.follower.many}`],
            ["Researchers", id.researcher ? `${id.researcher.one} / ${id.researcher.many}` : undefined],
          ]}
        />
        <p className={styles.quoteHead}>Sign-off</p>
        <blockquote className={styles.quote}>{id.sign_off}</blockquote>
        <p className={styles.quoteHead}>AI disclosure</p>
        <blockquote className={styles.quote}>{id.disclosure}</blockquote>
      </div>
    </Section>
  );
}

function VoiceFace({ d }: { d: CharacterDetail }) {
  const v = d.voice;
  const a = d.avatar;
  const hasVoice = v !== undefined && Object.keys(v).length > 0;
  const hasFace = a !== undefined && (a.rig !== undefined || a.face !== undefined);
  // The voice's fields live on the Voice tab (with this machine's override); here, only who speaks.
  const toVoice = (
    <RouteLink to={pkgRoute(d.id, "voice")} className={buttonClass("secondary", "compact")} data-role="characters-to-voice">
      Voice tab
    </RouteLink>
  );
  return (
    <Section title="Voice and face">
      {!hasVoice && !hasFace ? (
        <EmptyState title="The daemon's voice and face" hint={<>The package sets neither: the daemon&apos;s [voice] and [stage] settings apply. Pick a provider and voice on the {toVoice}.</>} />
      ) : (
        <div data-role="characters-voice">
          <Kv
            rows={[
              ["Provider", <>{PROVIDER_NAME[v?.provider ?? "kokoro"]}{v?.provider === undefined && <small className={l2.dim}> · unset, so Kokoro</small>} {toVoice}</>],
              ["Face", a?.face === undefined ? undefined : faceLabel(a.face)],
              ["Rig spec", a?.rig === undefined ? undefined : <code className={l2.code}>{a.rig}</code>],
            ]}
          />
          <p className={l2.note}>The voice, its speed and this machine&apos;s override are on the Voice tab. Each face field set here overrides the daemon&apos;s own setting; an unset field keeps it.</p>
        </div>
      )}
    </Section>
  );
}

function Ranks({ d }: { d: CharacterDetail }) {
  const o = d.rank_overrides;
  const titles = d.rank_titles as Partial<Record<Rank, string>>;
  const rows = RANK_ORDER.filter((r) => o?.weights?.[r] !== undefined || o?.thresholds?.[r] !== undefined);
  return (
    <Section title="Rank ladder">
      {!o || (rows.length === 0 && o.propose_min_rank === undefined) ? (
        <EmptyState title="The engine's ladder" hint="No [ranks] overrides: weights, thresholds and who may propose are the engine's." />
      ) : (
        <div data-role="characters-rank-overrides">
          {o.propose_min_rank !== undefined && <p className={l2.note}>Proposing a scheme takes {rankLabel(titles, o.propose_min_rank)} or higher.</p>}
          {rows.length > 0 && (
            <table className={styles.table}>
              <thead>
                <tr>
                  <th scope="col">Rank</th>
                  <th scope="col">Vote weight</th>
                  <th scope="col">Reached by</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const t = o.thresholds?.[r];
                  const w = o.weights?.[r];
                  const how = [
                    t?.min_gifted === undefined ? undefined : `${t.min_gifted} gifted`,
                    ...(t?.tiers ?? []).map((x) => `tier ${x.tier} for ${x.min_months ?? 0} months`),
                  ].filter((x): x is string => x !== undefined);
                  return (
                    <tr key={r}>
                      <th scope="row">{rankLabel(titles, r)}</th>
                      <td>{w === undefined ? "engine" : `×${w.toFixed(1)}`}</td>
                      <td>{how.length > 0 ? how.join(", or ") : "engine"}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
      )}
    </Section>
  );
}

const differs = (t: TuningRow) => t.value !== t.default;

function Tuning({ d }: { d: CharacterDetail }) {
  const rows = d.tuning ?? [];
  if (rows.length === 0) {
    return (
      <Section title="Tuning">
        <EmptyState title="No tuning table" hint="This daemon does not report the tuning table." />
      </Section>
    );
  }
  const changed = rows.filter(differs).length;
  return (
    <Section title="Tuning" right={<span className={styles.count}>{changed === 0 ? "every key at the engine default" : `${changed} of ${rows.length} differ from the default`}</span>}>
      <table className={styles.table} data-role="characters-tuning">
        <thead>
          <tr>
            <th scope="col">Lesson</th>
            <th scope="col">Value</th>
            <th scope="col">Default</th>
            <th scope="col">Provenance</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((t) => (
            <tr key={t.key} data-role="characters-tuning-row" data-differs={differs(t) ? "true" : undefined} className={differs(t) ? styles.differs : undefined}>
              <th scope="row">
                {tuningLabel(t.doc)}
                {!t.set && <small className={l2.dim}> · engine default</small>}
              </th>
              <td>
                {tuningValue(t.key, t.value)}
                {differs(t) && <span className={styles.badge}>changed</span>}
              </td>
              <td className={l2.dim}>{tuningValue(t.key, t.default)}</td>
              <td>
                <span className={styles.prov} data-prov={t.provenance}>
                  {PROVENANCE_NAME[t.provenance]}
                </span>
                {t.source !== undefined && <small className={styles.source}>{t.source}</small>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </Section>
  );
}

function Flavour({ d }: { d: CharacterDetail }) {
  const entries = Object.entries(d.segment_flavour ?? {}).filter((e) => e[1] !== undefined);
  return (
    <Section title="Segment flavour">
      {entries.length === 0 ? (
        <EmptyState title="No segment flavour" hint="Each segment's prompts run as the segment tables write them." />
      ) : (
        <div data-role="characters-flavour">
          {entries.map(([seg, f]) => (
            <div key={seg} className={styles.flavour}>
              <h3>{SEGMENT_NAME[seg as SegmentId] ?? "Unknown segment"}</h3>
              <Kv rows={[["Rules", f?.rules_prompt], ...Object.entries(f?.states ?? {}).map(([s, t]) => [stateLabel(s), t] as const)]} />
            </div>
          ))}
        </div>
      )}
    </Section>
  );
}

export function ParamsTab({ d }: { d: CharacterDetail }) {
  return (
    <>
      <Head kicker={KICKER} title="Parameters" lede="What character.toml sets over the engine's defaults, read-only. Change it in the package and validate it." />
      <Identity d={d} />
      <VoiceFace d={d} />
      <Ranks d={d} />
      <Tuning d={d} />
      <Flavour d={d} />
    </>
  );
}
