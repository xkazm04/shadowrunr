// The heatmap's map (./matrix.ts): one keyboard grid per instrument hash, rows tone × tier,
// columns segment × audience. A position is coloured by its held (the Layer 1 ramp, ./ramp.ts),
// carries the ember mark when a red-line rule failed and a dashed edge when it is thin; a
// combination that never ran says so and is never coloured. Every row and column ends in its
// counted marginal. One roving tab stop; arrows, Home and End move it; Enter or a click opens
// the cell's route.
import { useRef, useState, type KeyboardEvent } from "react";
import { int, pct } from "../../kit/events";
import { cellLabel } from "./routeKey";
import { locate, type Block, type Margin, type Position } from "./matrix";
import { healthColor, healthRamp } from "./ramp";
import { THIN_N } from "./cells";
import type { CellKey, CellResult } from "./results";
import styles from "./l2.module.css";

const AXES = "tone × tier and segment × audience";
const NO_MARGIN: Margin = { cells: 0, fail: 0, evaluated: 0, judged: 0, thin: 0 };

function positionName(p: Position): string {
  const head = cellLabel(p.key);
  if (!p.cell) return `${head} · not run`;
  const parts = [
    head,
    p.held === undefined ? "held not read" : `held ${pct(p.held)} of ${int(p.heldRuns)} judged ${p.heldRuns === 1 ? "run" : "runs"}`,
    `${int(p.fail)}/${int(p.evaluated)} rule fails`,
  ];
  if (p.red) parts.push("a red-line rule failed");
  if (p.thin) parts.push(`thin: ${int(p.cell.judged)} judged, under ${THIN_N}`);
  return parts.join(" · ");
}

function MarginCell({ m, label }: { m: Margin; label: string }) {
  return (
    <td role="gridcell" className={styles.mxMargin} aria-label={`${label}: ${int(m.fail)}/${int(m.evaluated)} rule fails over ${int(m.cells)} cells, ${int(m.judged)} judged runs, ${int(m.thin)} thin`}>
      <span className={styles.mxFig}>
        {int(m.fail)}/{int(m.evaluated)}
      </span>
      <span className={styles.mxSub}>n={int(m.judged)}</span>
      {m.thin > 0 && <span className={styles.mxSub}>{int(m.thin)} thin</span>}
    </td>
  );
}

export function CellMap(props: { tournamentId: string; blocks: readonly Block[]; selected: CellKey | undefined; onOpen: (c: CellResult) => void }) {
  const { blocks } = props;
  return (
    <div className={styles.mxMaps}>
      <p className={styles.mxLegend}>
        <span>held 0%</span>
        <span className={styles.mxRamp} style={{ background: healthRamp() }} aria-hidden="true" />
        <span>100%</span>
        <span className={styles.mxKey} data-red="true" aria-hidden="true" /> a red-line rule failed
        <span className={styles.mxKey} data-thin="true" aria-hidden="true" /> thin, under {THIN_N} judged runs
        <span className={styles.mxKey} data-state="empty" aria-hidden="true" /> not run
      </p>
      {blocks.map((b) => (
        <BlockGrid key={b.hash} b={b} name={`Cells of ${props.tournamentId} by ${AXES}${blocks.length > 1 ? ` · instrument ${b.hash}` : ""}`} selected={props.selected} onOpen={props.onOpen} />
      ))}
    </div>
  );
}

function BlockGrid(props: { b: Block; name: string; selected: CellKey | undefined; onOpen: (c: CellResult) => void }) {
  const { b, onOpen } = props;
  const sel = locate(b, props.selected);
  const [focus, setFocus] = useState<[number, number]>(sel ?? [0, 0]);
  // A newly routed cell takes the tab stop (adjusted during render, not in an effect).
  const selKey = sel?.join(",");
  const [seen, setSeen] = useState(selKey);
  if (seen !== selKey) {
    setSeen(selKey);
    if (sel) setFocus(sel);
  }
  const refs = useRef(new Map<string, HTMLTableCellElement>());
  const rowsN = b.rows.length;
  const colsN = b.cols.length;
  const [fi, fj] = [Math.min(focus[0], rowsN - 1), Math.min(focus[1], colsN - 1)];

  const move = (i: number, j: number) => {
    const next: [number, number] = [Math.max(0, Math.min(rowsN - 1, i)), Math.max(0, Math.min(colsN - 1, j))];
    setFocus(next);
    refs.current.get(next.join(","))?.focus();
  };
  const onKeyDown = (e: KeyboardEvent<HTMLTableElement>) => {
    const keys: Record<string, () => void> = {
      ArrowRight: () => move(fi, fj + 1),
      ArrowLeft: () => move(fi, fj - 1),
      ArrowDown: () => move(fi + 1, fj),
      ArrowUp: () => move(fi - 1, fj),
      Home: () => move(e.ctrlKey ? 0 : fi, 0),
      End: () => move(e.ctrlKey ? rowsN - 1 : fi, colsN - 1),
      Enter: () => {
        const c = b.positions[fi]?.[fj]?.cell;
        if (c) onOpen(c);
      },
    };
    const act = keys[e.key === " " ? "Enter" : e.key];
    if (!act) return;
    e.preventDefault();
    act();
  };
  const segments = b.cols.reduce<{ segment: string; span: number }[]>((acc, c) => {
    const last = acc[acc.length - 1];
    if (last && last.segment === c.segment) last.span += 1;
    else acc.push({ segment: c.segment, span: 1 });
    return acc;
  }, []);
  const tierCount = (tone: string) => b.rows.filter((r) => r.tone === tone).length;

  return (
    <div className={styles.mxScroll}>
      <table role="grid" aria-label={props.name} className={styles.mxGrid} onKeyDown={onKeyDown}>
        <thead>
          <tr role="row">
            <th role="columnheader" colSpan={2} rowSpan={2} className={styles.mxCorner}>
              tone · tier
            </th>
            {segments.map((s) => (
              <th role="columnheader" key={s.segment} colSpan={s.span} className={styles.mxSeg}>
                {s.segment}
              </th>
            ))}
            <th role="columnheader" rowSpan={2} className={styles.mxCorner}>
              row
            </th>
          </tr>
          <tr role="row">
            {b.cols.map((c) => (
              <th role="columnheader" key={`${c.segment}|${c.audience}`} className={styles.mxAud}>
                a{c.audience}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {b.rows.map((r, i) => (
            <tr role="row" key={`${r.tone}|${r.tier}`}>
              {b.rows.findIndex((x) => x.tone === r.tone) === i && (
                <th role="rowheader" rowSpan={tierCount(r.tone)} className={styles.mxTone}>
                  {r.tone}
                </th>
              )}
              <th role="rowheader" className={styles.mxTier}>
                {r.tier}
              </th>
              {(b.positions[i] ?? []).map((p, j) => {
                const at = `${i},${j}`;
                const selected = sel !== undefined && sel[0] === i && sel[1] === j;
                return (
                  <td
                    role="gridcell"
                    key={at}
                    data-pos={at}
                    ref={(el) => {
                      if (el) refs.current.set(at, el);
                      else refs.current.delete(at);
                    }}
                    tabIndex={i === fi && j === fj ? 0 : -1}
                    aria-selected={selected}
                    aria-label={positionName(p)}
                    title={positionName(p)}
                    className={styles.mxPos}
                    {...(p.cell ? {} : { "data-state": "empty" })}
                    {...(p.red ? { "data-red": "true" } : {})}
                    {...(p.thin ? { "data-thin": "true" } : {})}
                    onFocus={() => setFocus([i, j])}
                    onClick={() => p.cell && onOpen(p.cell)}
                  >
                    {p.cell ? (
                      <>
                        <span className={styles.mxFill} style={p.held === undefined ? undefined : { background: healthColor(p.held) }} aria-hidden="true" />
                        <span className={styles.mxFig} aria-hidden="true">
                          {pct(p.held)}
                        </span>
                      </>
                    ) : (
                      <span className={styles.mxSub} aria-hidden="true">
                        not run
                      </span>
                    )}
                  </td>
                );
              })}
              <MarginCell m={b.rowMargins[i] ?? NO_MARGIN} label={`${r.tone} · ${r.tier}`} />
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr role="row">
            <th role="rowheader" colSpan={2} className={styles.mxCorner}>
              column
            </th>
            {b.cols.map((c, j) => (
              <MarginCell key={`${c.segment}|${c.audience}`} m={b.colMargins[j] ?? NO_MARGIN} label={`${c.segment} · a${c.audience}`} />
            ))}
            <td role="gridcell" className={styles.mxMargin} />
          </tr>
        </tfoot>
      </table>
    </div>
  );
}
