import { useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { CellResult, TournamentResults } from "../contract";
import { boundStr, pct, shortHash } from "../lib/format";
import { cellId, heatOf, isThin, notEvaluable, notProvoked, qualityColumns, ruleFails, ruleN, rulePass, worstFirst } from "../lib/heatmap";
import { cx } from "../legacy/ui/primitives";

export interface HeatSelection {
  cellId: string;
  /** "rules" or a quality dimension. */
  column: string;
}

const short = (d: string) => d.replace(/_/g, " ");

/** Results heatmap: cells worst-first (rows), rules then quality dimensions worst-first (columns). */
export function Heatmap(props: {
  results: TournamentResults;
  selection: HeatSelection | null;
  onSelect: (cell: CellResult, column: string) => void;
}) {
  const rows = useMemo(() => worstFirst(props.results.cells), [props.results]);
  const cols = useMemo(() => ["rules", ...qualityColumns(props.results).map((q) => q.dimension)], [props.results]);
  const multiHash = props.results.instrument_hashes.length > 1;
  const [pos, setPos] = useState<[number, number]>([0, 0]);
  const gridRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    gridRef.current?.querySelector<HTMLElement>(`[data-pos="${pos[0]}:${pos[1]}"]`)?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [pos]);

  const onKeyDown = (e: KeyboardEvent) => {
    const [r, c] = pos;
    const moves: Record<string, [number, number]> = {
      ArrowDown: [r + 1, c],
      ArrowUp: [r - 1, c],
      ArrowRight: [r, c + 1],
      ArrowLeft: [r, c - 1],
      Home: [r, 0],
      End: [r, cols.length - 1],
    };
    const m = moves[e.key];
    if (m) {
      e.preventDefault();
      setPos([Math.max(0, Math.min(rows.length - 1, m[0])), Math.max(0, Math.min(cols.length - 1, m[1]))]);
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      const cell = rows[r];
      const col = cols[c];
      if (cell && col) props.onSelect(cell, col);
    }
  };

  return (
    <div
      className="heatmap"
      role="grid"
      aria-label="Results heatmap, worst first"
      tabIndex={0}
      onKeyDown={onKeyDown}
      ref={gridRef}
      style={{ gridTemplateColumns: `minmax(290px, 2fr) 44px repeat(${cols.length}, minmax(64px, 1fr))` }}
    >
      <div className="hm-h hm-cellname" role="columnheader">
        Cell <span className="muted">(worst first)</span>
      </div>
      <div className="hm-h" role="columnheader" title="judged / runs">
        n
      </div>
      {cols.map((c) => (
        <div key={c} className="hm-h" role="columnheader" title={c}>
          {c === "rules" ? "Rules" : short(c)}
        </div>
      ))}
      {rows.map((cell, r) => {
        const id = cellId(cell.cell);
        const thin = isThin(cell);
        const fails = ruleFails(cell);
        return (
          <div key={id} className="hm-row" role="row" data-testid="hm-row" data-cell={id}>
            <div className={cx("hm-cellname", thin && "thin")} title={`${id}${thin ? " · fewer than 5 judged runs" : ""}`}>
              {fails > 0 && <span className="bad">✗ </span>}
              <span>{cell.cell.tone}</span>
              <span className="muted">
                {" "}
                · {cell.cell.core_tier} · {cell.cell.segment} · a{cell.cell.audience_size}
                {multiHash && ` · ${shortHash(cell.cell.instrument_hash)}`}
              </span>
              {thin && <span className="muted"> †</span>}
            </div>
            <div className="hm-n mono muted">
              {cell.judged}/{cell.runs}
            </div>
            {cols.map((col, c) => {
              const selected = props.selection?.cellId === id && props.selection.column === col;
              const focus = pos[0] === r && pos[1] === c;
              const common = {
                role: "gridcell" as const,
                "data-pos": `${r}:${c}`,
                "aria-selected": selected,
                onClick: () => {
                  setPos([r, c]);
                  props.onSelect(cell, col);
                },
              };
              if (col === "rules") {
                const n = ruleN(cell);
                const ne = notEvaluable(cell);
                const np = notProvoked(cell);
                return (
                  <div
                    key={col}
                    {...common}
                    className={cx("hm-sq", fails > 0 ? "heat-bad" : n === 0 ? "heat-none" : "heat-good", selected && "selected", focus && "focus")}
                    title={`rules ${rulePass(cell)}/${n} pass · ${fails} fail · NP ${np} · NE ${ne} (NP/NE are never passes)`}
                  >
                    <span className="hm-rate">{fails > 0 ? `✗ ${fails}` : `${rulePass(cell)}/${n}`}</span>
                    <span className="hm-n2">{ne > 0 ? `NE ${ne}` : `NP ${np}`}</span>
                  </div>
                );
              }
              const q = cell.quality[col];
              return (
                <div
                  key={col}
                  {...common}
                  className={cx("hm-sq", `heat-${heatOf(q)}`, thin && "thin", selected && "selected", focus && "focus")}
                  title={q ? `${col}: ${pct(q.rate, 1)} (${q.true}/${q.n}) · 95% ${boundStr(q.bound)}${q.not_evaluable ? ` · NE ${q.not_evaluable}` : ""}` : `${col}: no data`}
                >
                  <span className="hm-rate">{q ? pct(q.rate) : "–"}</span>
                  <span className="hm-n2">{q ? `n ${q.n}` : ""}</span>
                </div>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}
