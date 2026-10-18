// Layer 1: the winner's cast poster (l1.js P3.L1.characters, l1.css .cb-*): the name plate, the
// portrait in its arch, the active tone, the roster selector and the special guests. Every
// summarising element is a lit link into the character package. Data: character.list for the
// roster, character.get for the selected character's foil names.
import { FoilArt, MalakorArt } from "@shadowrunr/stage-kit/ui";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { CharacterSummary } from "@shadowrunr/contract";
import { useConnection, useRpc } from "../../data";
import { PosterFrame } from "../../kit/PosterFrame";
import { pageRoute, RouteLink } from "../../router";
import { cx } from "../../ui/cx";
import { ART_CHARACTER, CharacterFace, GenericAvatar } from "./Avatar";
import styles from "./Layer1.module.css";
import { factsLine, foilName, foilShort, ROSTER_MANY, rosterKicker, rosterWindow, selectedIndex, shortName, stepIndex, toneLabel } from "./model";

export function Layer1({ active }: { readonly active: boolean }) {
  if (!active) return null;
  return <CastPoster />;
}

function CastPoster() {
  const list = useRpc("character.list", {});
  const conn = useConnection();
  const [chosen, setChosen] = useState<string | undefined>(undefined);
  const rows = list.data ?? [];
  const at = selectedIndex(rows, chosen);
  const cur = rows[at];
  const detail = useRpc("character.get", cur ? { id: cur.id } : null);

  let slip: { text: string; retry?: () => void } | undefined;
  if (list.data === undefined) {
    if (list.errorView) slip = { text: list.errorView.title, retry: list.reload };
    else slip = { text: conn.summary === "offline" ? "The daemon is not reachable. The cast shows here once it answers." : "Reading the cast…" };
  } else if (rows.length === 0) {
    slip = { text: "No characters yet. A character is a folder under characters/: add one and it takes the stage." };
  } else if (list.errorView) {
    slip = { text: `Showing the last good reply · ${list.errorView.title}`, retry: list.reload };
  }

  const pkg = (...sub: string[]) => (cur ? pageRoute("characters", cur.id, ...sub) : pageRoute("characters", "all"));
  const foilNames = new Map((detail.data?.foils ?? []).map((f) => [f.id, foilName(f)]));

  return (
    <PosterFrame
      className={styles.root}
      role="characters-layer1"
      measure
      credit={cur ? `Starring ${cur.name} · ` : "The cast · "}
      cta={{ to: cur ? pkg("package", "constitution") : pageRoute("characters", "all"), label: cur ? "Open the character package" : "Open all characters", role: "characters-cta" }}
    >
      <div className={styles.hero}>
        <RouteLink to={pkg()} className={cx(styles.hot, styles.name)} title="The character package: constitution, optional rules, bible, tones, foils, situations" data-role="characters-nameplate">
          {cur ? (
            <>
              <span className={styles.k}>{rosterKicker(at, rows.length)}</span>
              <span className={styles.n}>
                {cur.name.split(" ").map((w, i) => (
                  <i key={i}>{w}</i>
                ))}
              </span>
              {cur.tagline !== undefined && (
                <span className={styles.tg} data-role="characters-tagline">
                  {cur.tagline}
                </span>
              )}
              <span className={styles.r}>{factsLine(cur)}</span>
            </>
          ) : (
            <>
              <span className={styles.k}>The cast</span>
              <span className={styles.n}>
                <i>No</i>
                <i>cast</i>
              </span>
            </>
          )}
        </RouteLink>
        <RouteLink to={pkg()} className={cx(styles.hot, styles.portrait)} title="The character package" aria-label={cur ? `${cur.name}: open the package` : "All characters"} data-role="characters-portrait">
          <span className={styles.arch} data-role="characters-portrait-arch" />
          <span className={styles.fig}>
            {cur?.id === ART_CHARACTER ? (
              <MalakorArt />
            ) : cur ? (
              <span className={styles.avbig}>
                <GenericAvatar id={cur.id} />
              </span>
            ) : null}
          </span>
        </RouteLink>
        {cur && (
          <RouteLink to={pkg("package", "tones")} className={cx(styles.hot, styles.tone)} title="Tones, and what is still unmeasured" data-role="characters-tone">
            <b>Active tone</b>
            <span data-role="characters-active-tone">{toneLabel(cur.active_tone)}</span>
            <em>
              {cur.active_tone === cur.default_tone ? "The package default." : `The operator's choice; the package default is ${toneLabel(cur.default_tone)}.`} {cur.tones.length} tone{" "}
              {cur.tones.length === 1 ? "variant is" : "variants are"} the tournament axis.
            </em>
          </RouteLink>
        )}
        <Roster rows={rows} at={at} onPick={setChosen} />
        {cur && cur.foils.length > 0 && (
          <RouteLink to={pkg("package", "foils")} className={cx(styles.hot, styles.foils)} title="Special guests: the foils" data-role="characters-foils">
            <b>Special guests</b>
            {cur.foils.map((id) => (
              <span key={id} className={styles.fo}>
                <FoilArt id={id} />
                <em>{foilShort(foilNames.get(id) ?? foilName({ id }))}</em>
              </span>
            ))}
          </RouteLink>
        )}
        {slip && (
          <p className={styles.slip} role="status" data-role="characters-state">
            {slip.text}
            {slip.retry && (
              <button type="button" className={styles.slipAction} onClick={slip.retry}>
                Retry
              </button>
            )}
          </p>
        )}
      </div>
    </PosterFrame>
  );
}

/** The roster selector: arrow keys step through every character, the window follows. */
function Roster({ rows, at, onPick }: { rows: readonly CharacterSummary[]; at: number; onPick: (id: string) => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [focusId, setFocusId] = useState<string | undefined>(undefined);
  const many = rows.length > ROSTER_MANY;
  const w = rosterWindow(rows.length, at);

  useEffect(() => {
    if (focusId === undefined) return;
    const el = ref.current?.querySelector<HTMLElement>(`[data-id="${CSS.escape(focusId)}"]`);
    el?.focus();
  }, [focusId]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    // The roster owns its arrows; the keymap leaves a handled key alone.
    e.preventDefault();
    const next = rows[stepIndex(at, e.key === "ArrowRight" ? 1 : -1, rows.length)];
    if (!next) return;
    onPick(next.id);
    setFocusId(next.id);
  };

  return (
    <div ref={ref} className={cx(styles.roster, many && styles.many)} role="listbox" aria-label="Character selector" onKeyDown={onKeyDown} data-role="characters-roster">
      {rows.slice(w.start, w.end).map((c, k) => {
        const selected = w.start + k === at;
        return (
          <button
            key={c.id}
            type="button"
            role="option"
            aria-selected={selected}
            tabIndex={selected ? 0 : -1}
            data-id={c.id}
            className={styles.cr}
            title={c.name}
            onClick={() => onPick(c.id)}
            data-role="characters-roster-item"
          >
            <CharacterFace id={c.id} className={styles.av} />
            <em>{shortName(c.name)}</em>
          </button>
        );
      })}
      {w.more > 0 && (
        <RouteLink to={pageRoute("characters", "all")} className={cx(styles.cr, styles.more)} title="All characters" data-role="characters-roster-more">
          <span>+{w.more.toLocaleString("en-US")}</span>
          <em>All characters</em>
        </RouteLink>
      )}
      {!many && (
        <div className={cx(styles.cr, styles.slot)} aria-hidden="true">
          <span>+</span>
          <em>Your character is a folder</em>
        </div>
      )}
    </div>
  );
}
