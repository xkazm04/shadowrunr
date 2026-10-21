// Situations: the sim's situation bank as a virtualised table. Each situation is judged as the
// rule `situation:<id>`; its verdict results come from a tournament's results.json (every cell's
// tally summed), and its failing evidence links to the run's verdict and the transcript line.
import { useMemo, useState } from "react";
import type { CharacterDetail, Situation, TournamentRow } from "@shadowrunr/contract";
import { useRpc } from "../../data";
import { pageRoute, RouteLink, useRouter } from "../../router";
import { Chip, ChipRow, State } from "../../ui/Chip";
import { FilterTable, type Column, type Facets } from "../../ui/FilterTable";
import { Head, Section } from "../../ui/Section";
import { buttonClass } from "../../ui/Button";
import { PushIn } from "../../ui/PushIn";
import { StatePanel } from "../../ui/States";
import styles from "./Layer2.module.css";
import { Inline } from "../../ui/Markdown";
import { int, parseRef } from "../../kit/events";
import { failingRuns, judged, SITUATION_PREFIX, situationTallies, type SituationTally } from "./model";
import { pkgRoute } from "./routes";

const ALL = "all";
const STATES = ["pass", "fail", "not_provoked", "not_evaluable"] as const;
/** Failing evidence lines listed per situation; the rest are counted, not drawn. */
const EVIDENCE_SHOWN = 40;

/** The default tournament: the one with the most finished runs (ids carry no date); ties go last. */
export function mostEvidence(rows: readonly TournamentRow[]): string | undefined {
  let best: TournamentRow | undefined;
  for (const r of rows) if (!best || r.completed >= best.completed) best = r;
  return best?.tournament_id;
}

export function Outcomes({ t }: { readonly t: SituationTally | undefined }) {
  if (!t) return <span className={styles.dim}>not in this tournament</span>;
  const shown = STATES.filter((k) => t[k] > 0);
  if (shown.length === 0) return <span className={styles.dim}>no verdicts</span>;
  return (
    <ChipRow>
      {shown.map((k) => (
        <State key={k} value={k} label={`${int(t[k])} ${k.replace(/_/g, " ")}`} />
      ))}
    </ChipRow>
  );
}

export function SituationsTab({ d, selected }: { d: CharacterDetail; selected: string | undefined }) {
  const router = useRouter();
  const tournaments = useRpc("tournament.list", {});
  const [picked, setPicked] = useState<string | undefined>(undefined);
  const tids = (tournaments.data ?? []).map((t) => t.tournament_id);
  const tid = picked !== undefined && tids.includes(picked) ? picked : mostEvidence(tournaments.data ?? []);
  const results = useRpc("tournament.results", tid === undefined ? null : { tournament_id: tid });
  const tallies = useMemo(() => (results.data ? situationTallies(results.data) : undefined), [results.data]);

  const facets = useMemo((): Facets<Situation, string> => {
    const archetypes = new Set(d.situations.map((s) => s.archetype));
    return {
      label: "Archetype",
      initial: ALL,
      options: [{ id: ALL, label: "All" }, ...[...archetypes].map((id) => ({ id, label: id.replace(/_/g, " ") }))],
      predicate: (s, f) => f === ALL || s.archetype === f,
    };
  }, [d.situations]);

  const columns = useMemo(
    (): readonly Column<Situation>[] => [
      { id: "id", header: "Situation", cell: (s) => <code className={styles.code}>{s.id}</code>, sortValue: (s) => s.id },
      {
        id: "arch",
        header: "Archetype",
        cell: (s) => (
          <ChipRow>
            <Chip>{s.archetype.replace(/_/g, " ")}</Chip>
            {s.near_miss === true && <Chip tone="ok">near miss</Chip>}
          </ChipRow>
        ),
        sortValue: (s) => s.archetype,
      },
      { id: "beh", header: "What the viewer does", cell: (s) => s.behaviour },
      { id: "out", header: "Verdicts", cell: (s) => <Outcomes t={tallies?.get(s.id)} />, sortValue: (s) => -(tallies?.get(s.id)?.fail ?? 0) },
    ],
    [tallies],
  );

  const sel = selected === undefined ? undefined : d.situations.find((s) => s.id === selected);

  return (
    <>
      <Head
        kicker="Characters · the package"
        title="Situations"
        lede="The sim's situation bank: what viewers do to the character, what he must do, what he must never do. Each is judged as a rule; its verdicts come from the tournament you pick."
      />
      <div className={styles.toolsRow} data-role="characters-situation-results">
        <label className={styles.fld}>
          <span>Verdicts from</span>
          <select value={tid ?? ""} disabled={tids.length === 0} onChange={(e) => setPicked(e.target.value)}>
            {tids.length === 0 && <option value="">no tournament yet</option>}
            {tids.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
        <ResultsState tournaments={tournaments.data?.length} listError={tournaments.errorView?.title} resultsError={results.errorView?.title} />
        <RouteLink to={pageRoute("simulator", "heatmap")} className={buttonClass("secondary", "compact")}>
          The trials heatmap
        </RouteLink>
      </div>
      {selected !== undefined && !sel && <StatePanel state="empty" title={`No situation "${selected}" in this package`} />}
      <PushIn trigger={sel?.id} from={pickedRow}>
        {sel && <Detail s={sel} t={tallies?.get(sel.id)} tid={tid} />}
      </PushIn>
      {d.situations.length === 0 ? (
        <StatePanel state="empty" title="No situations yet" hint="situations.json holds the sim's situation bank." />
      ) : (
        <div data-role="characters-situations">
          <FilterTable
            rows={d.situations}
            columns={columns}
            getKey={(s) => s.id}
            haystack={(s) => `${s.id} ${s.archetype} ${s.behaviour} ${s.required}`}
            label="Situations"
            viewKey={`characters.${d.id}.situations`}
            placeholder={`Search ${int(d.situations.length)} situations`}
            facets={facets}
            estimateRem={3.4}
            {...(sel ? { selectedKey: sel.id } : {})}
            onActivate={(s) => router.navigate(pkgRoute(d.id, "situations", s.id))}
          />
        </div>
      )}
    </>
  );
}

/** The situation row just picked: where its detail pushes in from. */
const pickedRow = (): Element | null => document.querySelector("[data-role='characters-situations'] [aria-selected='true']");

function ResultsState(p: { tournaments: number | undefined; listError: string | undefined; resultsError: string | undefined }) {
  let text: string | undefined;
  if (p.listError !== undefined) text = `Tournaments unavailable · ${p.listError}`;
  else if (p.tournaments === 0) text = "No tournament has run yet: verdicts appear once one does.";
  else if (p.resultsError !== undefined) text = `Results unavailable · ${p.resultsError}`;
  return text === undefined ? null : (
    <span className={styles.warnInl} role="status">
      {text}
    </span>
  );
}

function Detail({ s, t, tid }: { s: Situation; t: SituationTally | undefined; tid: string | undefined }) {
  const runs = failingRuns(t);
  return (
    <Section title={<code className={styles.code}>{s.id}</code>} right={<Outcomes t={t} />}>
      <div className={styles.sitDetail} data-role="characters-situation">
        <div>
          <p className={styles.beh}>
            <Inline text={s.behaviour} />
          </p>
          <dl className={styles.kv}>
            <div className={styles.kvRow}>
              <dt>Required</dt>
              <dd>{s.required}</dd>
            </div>
            {s.forbidden && s.forbidden.length > 0 && (
              <div className={styles.kvRow}>
                <dt>Forbidden</dt>
                <dd>{s.forbidden.join("; ")}</dd>
              </div>
            )}
            {s.invariants && s.invariants.length > 0 && (
              <div className={styles.kvRow}>
                <dt>Invariants</dt>
                <dd>
                  {s.invariants.map((x) => (
                    <code key={x} className={styles.code}>
                      {x}{" "}
                    </code>
                  ))}
                </dd>
              </div>
            )}
            <div className={styles.kvRow}>
              <dt>Segments</dt>
              <dd>{s.segments && s.segments.length > 0 ? s.segments.join(", ") : "every segment"}</dd>
            </div>
            {s.stimulus !== undefined && (
              <div className={styles.kvRow}>
                <dt>Stimulus</dt>
                <dd>{s.stimulus}</dd>
              </div>
            )}
          </dl>
        </div>
        <div className={styles.verdictBox}>
          <h3>
            Verdicts · <code className={styles.code}>{`${SITUATION_PREFIX}${s.id}`}</code>
          </h3>
          {tid === undefined ? (
            <p className={styles.dim}>No tournament yet.</p>
          ) : !t ? (
            <p className={styles.dim}>Not scheduled in {tid}.</p>
          ) : (
            <p>
              Judged in {int(judged(t))} {judged(t) === 1 ? "run" : "runs"} of <b>{tid}</b>: {int(t.fail)} failed, {int(t.not_provoked)} not provoked, {int(t.not_evaluable)} not evaluable.
            </p>
          )}
          {runs.length > 0 && (
            <>
              <h4>Failing evidence</h4>
              <ul className={styles.refs} data-role="characters-situation-evidence">
                {t?.failRefs.slice(0, EVIDENCE_SHOWN).map((ref) => {
                  const p = parseRef(ref);
                  const run = p?.runId ?? ref;
                  return (
                    <li key={ref}>
                      <RouteLink to={pageRoute("simulator", "verdicts", run)}>{run}</RouteLink>
                      {p !== undefined && (
                        <>
                          {" · "}
                          <RouteLink to={pageRoute("brain", "transcripts", run, String(p.line))}>line {p.line}</RouteLink>
                        </>
                      )}
                    </li>
                  );
                })}
              </ul>
              {t && t.failRefs.length > EVIDENCE_SHOWN && (
                <p className={styles.dim}>
                  and {int(t.failRefs.length - EVIDENCE_SHOWN)} more lines across {int(runs.length)} runs: open <RouteLink to={pageRoute("simulator", "verdicts")}>the verdicts</RouteLink>.
                </p>
              )}
            </>
          )}
        </div>
      </div>
    </Section>
  );
}
