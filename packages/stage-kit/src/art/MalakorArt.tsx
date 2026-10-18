// Arch-Lich Malakor as an original screenprint (the Poster Wall's art.js `malakor`): bone,
// verdigris, a cracked crown and ember eyes, in the Weary Middle Manager slump (drooped lids,
// loosened tie, a mug). One component for every site that draws him (On Air, Characters).
// The colours are the illustration's own palette, kept exactly; the misregistered underprints
// are part of the print look. Class hooks for motion: mk-body, mk-head, mk-jaw, mk-eye,
// mk-lids, mk-mug, mk-steam.

import { useId } from "react";

export const ART = {
  bone: "#efe6d2",
  shade: "#cdbf9f",
  ink: "#0b0f0e",
  verd: "#2f7d6a",
  verdLight: "#86d3bb",
  ember: "#e8542a",
  emberLight: "#ffb15a",
  ochre: "#d6a85a",
  glow: "#fff0c0",
} as const;

const { bone: BONE, shade: SHADE, ink: INK, verd: VERD, verdLight: VERDL, ember: EMB, emberLight: EMBL } = ART;

/** A DOM-safe id prefix from React's useId (which may contain colons or guillemets). */
export function useArtId(): string {
  return `a${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;
}

const SKULL = "M200 74C120 74 94 136 97 196 99 236 115 258 131 276L142 308H258L269 276C285 258 301 236 303 196 306 136 280 74 200 74Z";
const ROBE = "M18 524C26 442 76 404 126 394L200 424 274 394C324 404 374 442 382 524Z";
const CROWN = "M104 134L110 78 148 108 168 52 200 98 232 52 252 108 290 78 296 134Z";
const ARM = "M262 470q-26 6-40 34M270 456q-30-4-46-24";

export function MalakorArt(props: { readonly tilt?: number; readonly className?: string }) {
  const p = useArtId();
  const ht = `url(#${p}ht)`;
  const tilt = props.tilt ?? -5;
  return (
    <svg
      className={props.className === undefined ? "mk" : `mk ${props.className}`}
      viewBox="0 0 400 520"
      role="img"
      aria-label="Stylised illustration of Arch-Lich Malakor, a bone and verdigris lich with a cracked crown and ember eyes"
    >
      <defs>
        <pattern id={`${p}ht`} width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(30)">
          <circle cx="3.5" cy="3.5" r="1.7" fill={INK} opacity=".38" />
        </pattern>
        <radialGradient id={`${p}gl`}>
          <stop offset="0" stopColor={ART.glow} />
          <stop offset=".3" stopColor={EMBL} />
          <stop offset=".7" stopColor={EMB} />
          <stop offset="1" stopColor={EMB} stopOpacity="0" />
        </radialGradient>
      </defs>
      <g className="mk-body">
        {/* robe, misregistered verdigris layer behind */}
        <path d={ROBE} fill={VERD} transform="translate(9 7)" />
        <path d={ROBE} fill={INK} />
        <path d="M126 394L200 424 274 394" fill="none" stroke={VERDL} strokeWidth="7" strokeLinejoin="round" />
        <path d="M52 470C80 440 120 430 150 436M350 470C322 440 282 430 252 436" stroke={VERD} strokeWidth="4" fill="none" />
        {/* necktie, loosened */}
        <path d="M192 420L214 424 226 470 200 512 178 468Z" fill={EMB} stroke={INK} strokeWidth="4" strokeLinejoin="round" />
        <path d="M196 428L212 430 214 452 198 448Z" fill={EMBL} opacity=".6" />
        {/* pauldrons */}
        <path d="M104 410L52 270 150 352Z" fill={INK} stroke={VERD} strokeWidth="5" strokeLinejoin="round" />
        <path d="M296 410L348 270 250 352Z" fill={INK} stroke={VERD} strokeWidth="5" strokeLinejoin="round" />
        {/* hand and mug */}
        <g className="mk-mug">
          <rect x="282" y="418" width="64" height="62" rx="6" fill={BONE} stroke={INK} strokeWidth="5" />
          <path d="M346 432h18a14 14 0 0 1 0 34h-18" fill="none" stroke={INK} strokeWidth="6" />
          <rect x="282" y="438" width="64" height="12" fill={EMB} />
          <path className="mk-steam" d="M300 408q-8-12 0-22t0-20M322 408q-8-12 0-22t0-20" fill="none" stroke={BONE} strokeWidth="4" strokeLinecap="round" opacity=".8" />
        </g>
        <path d={ARM} fill="none" stroke={BONE} strokeWidth="12" strokeLinecap="round" />
        <path d={ARM} fill="none" stroke={INK} strokeWidth="3" strokeLinecap="round" strokeDasharray="2 14" />
        <g className="mk-head" transform={`rotate(${tilt} 200 330)`}>
          {/* misregistered ember underprint */}
          <path d={SKULL} fill={EMB} transform="translate(11 9)" />
          <g className="mk-jaw">
            <path d="M140 292Q200 356 260 292L254 336Q200 384 146 336Z" fill={BONE} stroke={INK} strokeWidth="5" strokeLinejoin="round" />
            <path d="M146 336Q200 384 254 336L252 322Q200 366 148 322Z" fill={ht} />
            <g fill={BONE} stroke={INK} strokeWidth="3">
              <rect x="160" y="310" width="13" height="16" rx="2" />
              <rect x="175" y="314" width="13" height="16" rx="2" />
              <rect x="190" y="316" width="13" height="16" rx="2" />
              <rect x="205" y="316" width="13" height="16" rx="2" />
              <rect x="220" y="314" width="13" height="16" rx="2" />
              <rect x="235" y="310" width="12" height="16" rx="2" />
            </g>
          </g>
          <path d={SKULL} fill={BONE} stroke={INK} strokeWidth="6" strokeLinejoin="round" />
          <path d="M258 308L269 276C285 258 301 236 303 196 306 150 296 112 270 92 296 140 292 236 258 308Z" fill={SHADE} />
          <path d="M97 196C99 236 115 258 131 276L142 308 150 270C120 262 104 232 104 196Z" fill={ht} />
          <path d="M122 250Q138 276 154 268M278 250Q262 276 246 268" stroke={INK} strokeWidth="3" fill="none" opacity=".55" />
          {/* sockets and eyes with weary lids */}
          <path d="M124 196Q146 164 186 184Q192 232 158 240Q124 236 124 196Z" fill={INK} />
          <path d="M276 196Q254 164 214 184Q208 232 242 240Q276 236 276 196Z" fill={INK} />
          <g className="mk-eye">
            <circle cx="158" cy="210" r="26" fill={`url(#${p}gl)`} opacity=".9" />
            <circle cx="158" cy="210" r="10" fill={ART.glow} />
            <circle cx="242" cy="210" r="26" fill={`url(#${p}gl)`} opacity=".9" />
            <circle cx="242" cy="210" r="10" fill={ART.glow} />
          </g>
          <g className="mk-lids">
            <path d="M126 190Q158 170 190 186L188 206Q158 196 128 208Z" fill={INK} />
            <path d="M274 190Q242 170 210 186L212 206Q242 196 272 208Z" fill={INK} />
            <path d="M130 206Q158 214 188 205M270 206Q242 214 212 205" stroke={BONE} strokeWidth="3" fill="none" opacity=".75" />
          </g>
          <path d="M138 246Q158 256 180 244M262 246Q242 256 220 244" stroke={INK} strokeWidth="4" fill="none" opacity=".5" />
          <path d="M200 236L186 268Q200 276 214 268Z" fill={INK} />
          <path d="M170 288Q200 298 230 288" stroke={INK} strokeWidth="4" fill="none" />
          {/* crown, verdigris, cracked */}
          <path d={CROWN} fill={VERD} transform="translate(7 6)" />
          <path d={CROWN} fill={VERDL} stroke={INK} strokeWidth="6" strokeLinejoin="round" />
          <path d={CROWN} fill={ht} opacity=".7" />
          <path d="M102 124Q200 104 298 124L295 146Q200 126 105 146Z" fill={VERD} stroke={INK} strokeWidth="6" strokeLinejoin="round" />
          <path d="M200 98L194 118 206 130 198 146" stroke={INK} strokeWidth="5" fill="none" strokeLinejoin="round" />
          <path d="M232 52L228 72 238 82" stroke={INK} strokeWidth="4" fill="none" />
          <circle cx="200" cy="130" r="8" fill={EMB} stroke={INK} strokeWidth="3" />
          <circle cx="150" cy="136" r="5" fill={EMBL} />
          <circle cx="250" cy="136" r="5" fill={EMBL} />
        </g>
      </g>
    </svg>
  );
}
