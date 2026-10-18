// Six original rank sigils, Thrall to Dread Council. Each sits on the same faceted seal so a
// rank reads by silhouette first and colour second (colour-blind safe).

import type { Rank } from "@shadowrunr/contract";
import { RANK_COLOR, RANK_TITLE } from "../lore";

const SEAL = "M32 3 L56 15 L56 41 L32 61 L8 41 L8 15 Z";

function Glyph({ rank }: { rank: Rank }) {
  switch (rank) {
    case "thrall":
      // A broken shackle ring: bound, but only just.
      return (
        <g fill="none" strokeWidth="4" strokeLinecap="round">
          <path d="M22 38 A11 11 0 1 1 42 38" />
          <path d="M26 44 L28 40 M38 44 L36 40" />
          <circle cx="32" cy="46" r="2.5" fill="currentColor" stroke="none" />
        </g>
      );
    case "acolyte":
      // A single candle flame inside an open circle.
      return (
        <g>
          <circle cx="32" cy="33" r="15" fill="none" strokeWidth="2.5" strokeDasharray="4 3" />
          <path d="M32 20 C38 28 38 34 32 40 C26 34 26 28 32 20 Z" fill="currentColor" stroke="none" />
          <rect x="29.5" y="40" width="5" height="8" fill="currentColor" stroke="none" />
        </g>
      );
    case "cultist":
      // A triangle with a watching eye.
      return (
        <g fill="none" strokeWidth="3" strokeLinejoin="round">
          <path d="M32 16 L49 45 L15 45 Z" />
          <path d="M22 36 Q32 28 42 36 Q32 44 22 36 Z" />
          <circle cx="32" cy="36" r="3" fill="currentColor" stroke="none" />
        </g>
      );
    case "warlock":
      // A crescent cradling a four-point star.
      return (
        <g>
          <path d="M40 17 A16 16 0 1 0 40 49 A12 12 0 1 1 40 17 Z" fill="currentColor" stroke="none" />
          <path d="M40 26 L42 32 L48 34 L42 36 L40 42 L38 36 L32 34 L38 32 Z" fill="currentColor" stroke="none" />
        </g>
      );
    case "lich":
      // A phylactery gem under three crown points.
      return (
        <g strokeWidth="2.5" strokeLinejoin="round">
          <path d="M20 24 L24 14 L28 22 L32 12 L36 22 L40 14 L44 24 Z" fill="currentColor" stroke="none" />
          <path d="M32 27 L43 38 L32 50 L21 38 Z" fill="none" />
          <path d="M32 32 L37 38 L32 44 L27 38 Z" fill="currentColor" stroke="none" />
        </g>
      );
    case "dread_council":
      // Three obsidian spires over a ring of council seats.
      return (
        <g strokeWidth="2.5">
          <path d="M24 30 L27 13 L30 30 Z M29 30 L32 9 L35 30 Z M34 30 L37 13 L40 30 Z" fill="currentColor" stroke="none" />
          <circle cx="32" cy="40" r="10" fill="none" />
          {[0, 60, 120, 180, 240, 300].map((a) => (
            <circle
              key={a}
              cx={32 + 10 * Math.cos((a * Math.PI) / 180)}
              cy={40 + 10 * Math.sin((a * Math.PI) / 180)}
              r="2.4"
              fill="currentColor"
              stroke="none"
            />
          ))}
          <circle cx="32" cy="40" r="3.5" fill="currentColor" stroke="none" />
        </g>
      );
  }
}

export function Sigil({ rank, size = 32, title = true }: { rank: Rank; size?: number; title?: boolean }) {
  const color = RANK_COLOR[rank];
  const high = rank === "lich" || rank === "dread_council";
  return (
    <svg
      className={`sigil sigil-${rank}`}
      width={size}
      height={size}
      viewBox="0 0 64 64"
      role="img"
      aria-label={RANK_TITLE[rank]}
      style={{ color }}
    >
      {title && <title>{RANK_TITLE[rank]}</title>}
      <path d={SEAL} fill="#0c0f0e" stroke="currentColor" strokeWidth={high ? 3.5 : 2.5} opacity={0.95} />
      {high && <path d={SEAL} fill="none" stroke="currentColor" strokeWidth="1" transform="translate(32 32) scale(0.86) translate(-32 -32)" opacity={0.6} />}
      <g stroke="currentColor">
        <Glyph rank={rank} />
      </g>
    </svg>
  );
}
