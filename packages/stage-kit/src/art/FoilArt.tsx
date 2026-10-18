// The five guest foils as flat screenprints, 120x140 (the Poster Wall's art.js `foil`).
// Original parody archetypes, never real people. Colours are the illustration palette, kept
// exactly. An unknown foil id draws nothing (the caller shows the name instead).

import type { ReactElement } from "react";
import { ART } from "./MalakorArt";

const { bone: BONE, ink: INK, verd: VERD, verdLight: VERDL, ember: EMB, emberLight: EMBL, ochre: OCH } = ART;
const SKIN = "#d9b48f";

const DISRUPTOR = (
  <>
    <path d="M16 140C20 104 40 96 60 96s40 8 44 44Z" fill={INK} />
    <path d="M26 74C26 30 94 30 94 74L94 100H26Z" fill={INK} />
    <ellipse cx="60" cy="76" rx="26" ry="30" fill={SKIN} />
    <rect x="30" y="66" width="60" height="14" rx="7" fill={EMB} />
    <rect x="34" y="70" width="52" height="3" fill={BONE} opacity=".7" />
    <path d="M48 96Q60 104 72 96" stroke={INK} strokeWidth="3" fill="none" />
    <path d="M96 40l14-26 4 22z" fill={OCH} stroke={INK} strokeWidth="2" />
    <path d="M44 112h32" stroke={VERDL} strokeWidth="4" />
  </>
);

const LIFE_COACH = (
  <>
    <g stroke={OCH} strokeWidth="5" strokeLinecap="round">
      <path d="M60 8V0M24 20L18 12M96 20l6-8M8 52H0M112 52h8" />
    </g>
    <circle cx="60" cy="52" r="42" fill={OCH} opacity=".85" />
    <path d="M20 140C24 108 42 100 60 100s36 8 40 40Z" fill={VERD} />
    <ellipse cx="60" cy="56" rx="27" ry="31" fill="#e5c3a1" />
    <path d="M33 44Q60 26 87 44" stroke={VERD} strokeWidth="9" fill="none" strokeLinecap="round" />
    <circle cx="50" cy="56" r="3" fill={INK} />
    <circle cx="70" cy="56" r="3" fill={INK} />
    <path d="M44 70Q60 90 76 70Z" fill={INK} />
    <path d="M48 72Q60 80 72 72" fill={BONE} />
  </>
);

const HAS_BEEN_HERO = (
  <>
    <path d="M4 140L22 64 60 84 98 64 116 140Z" fill={EMB} />
    <path d="M22 64L60 84 98 64" fill="none" stroke={INK} strokeWidth="3" />
    <path d="M30 140C32 108 44 100 60 100s28 8 30 40Z" fill={INK} />
    <rect x="34" y="28" width="52" height="64" rx="22" fill={SKIN} />
    <path d="M34 52H86V62H34Z" fill={INK} />
    <circle cx="48" cy="57" r="3" fill={BONE} />
    <circle cx="72" cy="57" r="3" fill={BONE} />
    <path d="M44 80Q60 74 76 80" stroke={INK} strokeWidth="3" fill="none" />
    <rect x="72" y="104" width="30" height="14" rx="7" fill={VERDL} stroke={INK} strokeWidth="2.5" />
    <path d="M38 24Q60 6 82 24" stroke="#8d8576" strokeWidth="7" fill="none" strokeLinecap="round" />
  </>
);

const INSPECTOR = (
  <>
    <path d="M18 140C22 106 40 98 60 98s38 8 42 42Z" fill="#35403d" />
    <ellipse cx="60" cy="64" rx="25" ry="29" fill={SKIN} />
    <path d="M26 44Q60 18 94 44L98 52H22Z" fill={INK} />
    <rect x="22" y="46" width="76" height="8" rx="3" fill={INK} />
    <circle cx="60" cy="36" r="6" fill={OCH} />
    <circle cx="49" cy="66" r="9" fill="none" stroke={INK} strokeWidth="3" />
    <circle cx="71" cy="66" r="9" fill="none" stroke={INK} strokeWidth="3" />
    <path d="M58 66h4" stroke={INK} strokeWidth="3" />
    <path d="M48 84H72" stroke={INK} strokeWidth="3" />
    <g transform="rotate(8 92 112)">
      <rect x="70" y="86" width="40" height="52" fill={BONE} stroke={INK} strokeWidth="3" />
      <path d="M78 100h24M78 110h24M78 120h16" stroke={INK} strokeWidth="3" />
      <rect x="82" y="82" width="16" height="8" fill={INK} />
    </g>
  </>
);

const COIN_PROPHET = (
  <>
    <circle cx="60" cy="54" r="44" fill={OCH} />
    <circle cx="60" cy="54" r="34" fill="none" stroke={INK} strokeWidth="3" />
    <path d="M60 54m0 0a6 6 0 0 1 12 0 12 12 0 0 1-24 0 18 18 0 0 1 36 0" fill="none" stroke={INK} strokeWidth="2.5" opacity=".6" />
    <path d="M14 140C18 100 38 90 60 90s42 10 46 50Z" fill={VERD} />
    <path d="M34 70Q34 28 60 28T86 70L80 96H40Z" fill="#23463f" />
    <ellipse cx="60" cy="68" rx="19" ry="22" fill="#1a2420" />
    <circle cx="52" cy="66" r="3.5" fill={EMBL} />
    <circle cx="68" cy="66" r="3.5" fill={EMBL} />
    <path d="M52 78Q60 83 68 78" stroke={OCH} strokeWidth="3" fill="none" />
  </>
);

const FOILS: Readonly<Record<string, ReactElement>> = {
  "the-disruptor": DISRUPTOR,
  "the-life-coach": LIFE_COACH,
  "the-has-been-hero": HAS_BEEN_HERO,
  "the-inspector": INSPECTOR,
  "the-coin-prophet": COIN_PROPHET,
};

/** The foil ids that have art, in the winner's order. */
export const FOIL_ART_IDS: readonly string[] = Object.keys(FOILS);

export function hasFoilArt(id: string): boolean {
  return Object.hasOwn(FOILS, id);
}

export function FoilArt(props: { readonly id: string; readonly className?: string }) {
  const body = FOILS[props.id];
  if (body === undefined) return null;
  return (
    <svg
      className={props.className === undefined ? "foil-art" : `foil-art ${props.className}`}
      viewBox="0 0 120 140"
      role="img"
      aria-label="Stylised illustration of a foil character"
    >
      {body}
    </svg>
  );
}
