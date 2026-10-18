import { Suspense, type CSSProperties } from "react";
import { RouteLink, type Crumb, type PageId } from "../router";
import type { Rect, View } from "./layout";
import { INTRO } from "./Intro";
import styles from "./Poster.module.css";
import { pageNumber } from "./posters";
import type { PageModule, PosterCopy } from "./types";

export interface PosterProps {
  readonly id: PageId;
  readonly index: number;
  readonly copy: PosterCopy;
  readonly module: PageModule;
  readonly view: View;
  readonly active: boolean;
  readonly big: boolean;
  readonly dim: boolean;
  readonly railOpen: boolean;
  readonly pre: boolean;
  readonly still: boolean;
  readonly narrow: boolean;
  readonly rect: Rect | undefined;
  readonly bodyWidth: number;
  readonly crumbs: readonly Crumb[];
  readonly onOpen: () => void;
  readonly onBack: () => void;
  /** Receives the poster's open button, so arrow keys can move focus along the wall. */
  readonly registerHit: (el: HTMLButtonElement | null) => void;
}

function Face({ copy, num, module }: { copy: PosterCopy; num: string; module: PageModule }) {
  const { Motif, Glance } = module;
  return (
    <div className={styles.face} data-role="poster-face">
      <div className={styles.halftoneA} data-role="poster-halftone-a" />
      <div className={styles.halftoneB} data-role="poster-halftone-b" />
      {Motif && (
        <div className={styles.motif}>
          <Motif />
        </div>
      )}
      <i className={`${styles.reg} ${styles.regTl}`} />
      <i className={`${styles.reg} ${styles.regBr}`} />
      <div className={styles.num}>{num}</div>
      <div className={styles.vertical} aria-hidden="true">
        <span>{copy.name}</span>
        <span className={styles.short}>{copy.short}</span>
        <small>{copy.tag}</small>
      </div>
      <div className={styles.big}>
        <div className={styles.bigKicker}>
          {num} · {copy.name}
        </div>
        <div className={styles.bigTitle}>
          {copy.lines.map((l) => (
            <span key={l}>{l}</span>
          ))}
        </div>
        <div className={styles.glance}>{Glance && <Glance />}</div>
      </div>
    </div>
  );
}

function Spine(props: { copy: PosterCopy; num: string; module: PageModule; crumbs: readonly Crumb[]; onBack: () => void }) {
  const { Motif } = props.module;
  return (
    <div className={styles.spine} data-role="poster-spine">
      {Motif && (
        <div className={styles.spineMotif}>
          <Motif />
        </div>
      )}
      <button type="button" className={styles.back} onClick={props.onBack}>
        <i />
        Back
      </button>
      <nav className={styles.crumbs} aria-label="Breadcrumb">
        {props.crumbs.map((c, i) =>
          c.current ? (
            <b key={i} aria-current="page">
              {c.label}
            </b>
          ) : (
            <span key={i}>
              <RouteLink to={c.route}>{c.label}</RouteLink> <span className={styles.sep}>/</span>
            </span>
          ),
        )}
      </nav>
      <div className={styles.spineTitle}>
        <small>
          {props.num} / 06 · {props.copy.tag}
        </small>
        {props.copy.name}
      </div>
      <p className={styles.spineNote}>v0.1 · media-less phase. Voice, avatar and broadcast come next.</p>
    </div>
  );
}

export function Poster({ registerHit, ...p }: PosterProps) {
  const num = pageNumber(p.id);
  const { Layer1 } = p.module;
  const r = p.rect;
  const style = {
    ...(r ? { left: r.left, top: r.top, width: r.width, height: r.height } : {}),
    ...(p.pre ? { "--slam-delay": `${INTRO.slamStartMs + p.index * INTRO.slamStepMs}ms` } : {}),
  } as CSSProperties;
  return (
    <section
      className={styles.poster}
      id={`poster-${p.id}`}
      data-page={p.id}
      data-role="poster"
      data-view={p.view}
      data-active={p.active}
      data-big={p.big}
      data-dim={p.dim}
      data-rail={p.railOpen}
      data-pre={p.pre}
      data-alt={p.index % 2 === 1}
      data-still={p.still}
      data-narrow={p.narrow}
      style={style}
      aria-label={`${num} · ${p.copy.name}`}
    >
      <Face copy={p.copy} num={num} module={p.module} />
      <div className={styles.body} style={{ width: p.bodyWidth }} data-role="layer1">
        {p.view !== "wall" && p.active && (
          <Suspense fallback={null}>
            <Layer1 active={p.view === "l1"} />
          </Suspense>
        )}
      </div>
      {p.active && p.view === "l2" && <Spine copy={p.copy} num={num} module={p.module} crumbs={p.crumbs} onBack={p.onBack} />}
      <button type="button" ref={registerHit} className={styles.hit} aria-label={`Open ${p.copy.name}: ${p.copy.tag}`} onClick={p.onOpen} />
    </section>
  );
}
