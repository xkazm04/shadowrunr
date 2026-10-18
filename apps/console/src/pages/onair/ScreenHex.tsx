// The screen's case map (the winner's stage.js `hexSVG`): pointy-top hexes of circumradius 25,
// links between them, the hex number and, once revealed, its grade. Geometry comes from
// stage-kit; a transcript without a case file is placed procedurally from its revealed ids.
import type { Case, EvidenceGrade } from "@shadowrunr/contract";
import { axialToPixel, buildMap, hexPointsAt } from "@shadowrunr/stage-kit";
import { memo, useMemo } from "react";
import styles from "./Screen.module.css";

const S = 25;

interface Placed {
  readonly id: string;
  readonly x: number;
  readonly y: number;
}

function layout(caseFile: Case | null, revealed: readonly string[]) {
  const map = buildMap(caseFile, revealed);
  const pts: Placed[] = map.hexes.map((h) => ({ id: h.id, ...axialToPixel(h.q, h.r, S) }));
  if (pts.length === 0) return null;
  const xs = pts.map((p) => p.x);
  const ys = pts.map((p) => p.y);
  const minx = Math.min(...xs) - S;
  const maxx = Math.max(...xs) + S;
  const miny = Math.min(...ys) - S;
  const maxy = Math.max(...ys) + S;
  const byId = new Map(pts.map((p) => [p.id, p]));
  const links = map.links.flatMap(([a, b]) => {
    const pa = byId.get(a);
    const pb = byId.get(b);
    return pa && pb ? [{ key: `${a}-${b}`, a: pa, b: pb }] : [];
  });
  const viewBox = [minx, miny, maxx - minx, maxy - miny].map((n) => n.toFixed(1)).join(" ");
  return { pts, links, viewBox };
}

export const ScreenHex = memo(function ScreenHex(props: {
  caseFile: Case | null;
  revealed: readonly string[];
  grades: Readonly<Record<string, EvidenceGrade>>;
}) {
  const revealedKey = props.revealed.join(",");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `revealedKey` is `revealed` by value
  const drawn = useMemo(() => layout(props.caseFile, props.revealed), [props.caseFile, revealedKey]);
  if (!drawn) return <svg className={styles.hexsvg} viewBox="0 0 10 10" aria-hidden="true" />;
  const n = drawn.pts.length;
  return (
    <svg
      className={styles.hexsvg}
      viewBox={drawn.viewBox}
      role="img"
      aria-label={`${props.caseFile?.fictional === false ? "Case" : "Fictional demo case"}: ${n} hexes, graded DOC, REP, CLM or SPC as they are revealed`}
      data-role="onair-hexmap"
    >
      <g className={styles.hl}>
        {drawn.links.map((l) => (
          <line key={l.key} x1={l.a.x.toFixed(1)} y1={l.a.y.toFixed(1)} x2={l.b.x.toFixed(1)} y2={l.b.y.toFixed(1)} />
        ))}
      </g>
      {drawn.pts.map((p) => {
        const g = props.grades[p.id];
        return (
          <g key={p.id} className={styles.hx} data-id={p.id} data-g={g}>
            <polygon points={hexPointsAt(p.x, p.y, S - 1.5)} />
            <text x={p.x.toFixed(1)} y={(p.y + 4.5).toFixed(1)} fontSize="11">
              {p.id.replace("HEX-", "")}
            </text>
            <text className={styles.gr} x={p.x.toFixed(1)} y={(p.y + 15).toFixed(1)} fontSize="9">
              {g ?? ""}
            </text>
          </g>
        );
      })}
    </svg>
  );
});
