// Character faces. Malakor is drawn by stage-kit's one MalakorArt; a character with a portrait
// (the moderator) shows it in the same 120x140 tile; every other character gets the winner's
// generic printed avatar (sr-kit avatarSVG, 120x140), painted from a spec seeded by its id. The
// colours are the illustration's own palette, kept exactly.
import { MalakorArt } from "@shadowrunr/stage-kit/ui";
import type { ReactElement } from "react";
import { cx } from "../../ui/cx";
import styles from "./Avatar.module.css";
import vellum from "./art/vellum.webp";
import { avatarOf, type AvatarSpec } from "./model";

const BONE = "#e7dcc3";
const SHADE = "#b9ab8b";
const INK = "#10110f";

/** The character whose package carries bespoke art. */
export const ART_CHARACTER = "malakor";

/** 2D portraits (240x280, the tile's 120:140), by package id: the owner's picked illustrations. */
const PORTRAITS: Readonly<Record<string, { src: string; alt: string }>> = {
  moderator: { src: vellum, alt: "Clerk-Bailiff Vellum" },
};

const HEADS: readonly ReactElement[] = [
  <path key="h0" d="M60 18C34 18 22 40 24 62c1 14 8 22 16 28v14h40V90c8-6 15-14 16-28 2-22-10-44-36-44z" />,
  <path key="h1" d="M60 16C36 16 24 34 24 58c0 22 12 38 36 46 24-8 36-24 36-46 0-24-12-42-36-42z" />,
  <rect key="h2" x="26" y="20" width="68" height="84" rx="30" />,
  <path key="h3" d="M60 14L98 44 90 98 60 108 30 98 22 44z" />,
  <ellipse key="h4" cx="60" cy="62" rx="38" ry="46" />,
  <path key="h5" d="M60 16C38 16 26 36 26 56c0 10 3 18 8 24l4 24h44l4-24c5-6 8-14 8-24 0-20-12-40-34-40z" />,
];

function gear(a: AvatarSpec): ReactElement | null {
  switch (a.gear) {
    case 0:
      return <path d="M24 36L30 8l16 18 14-22 14 22 16-18 6 28z" fill={a.accent} stroke={INK} strokeWidth="2.5" strokeLinejoin="round" />;
    case 1:
      return <path d="M18 46C20 18 44 8 60 8s40 10 42 38c-14-10-28-14-42-14S32 36 18 46z" fill={a.deep} stroke={a.accent} strokeWidth="2.5" />;
    case 2:
      return (
        <>
          <path d="M22 40c4-22 20-32 38-32s34 10 38 32L60 30z" fill={a.accent} />
          <path d="M22 40c-8-6-12-16-8-26 6 4 12 10 14 18zM98 40c8-6 12-16 8-26-6 4-12 10-14 18z" fill={BONE} stroke={SHADE} strokeWidth="2" />
        </>
      );
    case 3:
      return (
        <>
          <path d="M16 42h88l-10-8-12-26H38L26 34z" fill={a.deep} stroke={a.accent2} strokeWidth="2.5" strokeLinejoin="round" />
          <circle cx="60" cy="24" r="6" fill={a.accent2} />
        </>
      );
    case 4:
      return (
        <>
          <path d="M20 50C20 22 40 6 60 6s40 16 40 44c-10-8-22-14-40-14S30 42 20 50z" fill={a.accent} opacity=".9" />
          <circle cx="60" cy="6" r="6" fill={a.accent2} />
        </>
      );
    case 5:
      return <circle cx="60" cy="14" r="26" fill="none" stroke={a.accent2} strokeWidth="4" opacity=".85" />;
    case 6:
      return <path d="M24 44L14 12l24 14 22-20 22 20 24-14-10 32z" fill={a.deep} stroke={a.accent} strokeWidth="2.5" strokeLinejoin="round" />;
    default:
      return null;
  }
}

function eyes(a: AvatarSpec): ReactElement | null {
  switch (a.eyes) {
    case 0:
      return (
        <>
          <circle cx="46" cy="60" r="8" fill={a.accent2} />
          <circle cx="74" cy="60" r="8" fill={a.accent2} />
        </>
      );
    case 1:
      return (
        <>
          <ellipse cx="46" cy="60" rx="9" ry="6" fill={a.accent2} />
          <ellipse cx="74" cy="60" rx="9" ry="6" fill={a.accent2} />
        </>
      );
    case 2:
      return (
        <>
          <circle cx="60" cy="56" r="13" fill={a.accent2} />
          <circle cx="60" cy="56" r="5" fill={INK} />
        </>
      );
    case 3:
      return <path d="M36 62l16-8 2 12zM84 62L68 54l-2 12z" fill={a.accent2} />;
    case 4:
      return (
        <>
          <circle cx="46" cy="60" r="7" fill={a.accent2} />
          <circle cx="74" cy="60" r="4" fill={a.accent2} />
          <path d="M62 54l12-6" stroke={INK} strokeWidth="3" />
        </>
      );
    default:
      return null;
  }
}

function mark(a: AvatarSpec): ReactElement | null {
  switch (a.mark) {
    case 1:
      return <path d="M82 40l-14 28" stroke={INK} strokeWidth="2.5" opacity=".7" />;
    case 2:
      return <circle cx="74" cy="60" r="14" fill="none" stroke={a.accent} strokeWidth="3" />;
    case 3:
      return <path d="M44 86h32M48 94h24" stroke={INK} strokeWidth="3" opacity=".6" />;
    case 4:
      return <path d="M50 28l20 0M54 34l12 0" stroke={SHADE} strokeWidth="3" />;
    default:
      return null;
  }
}

/** The generic printed avatar for a character without bespoke art. */
export function GenericAvatar({ id, className }: { readonly id: string; readonly className?: string }) {
  const portrait = PORTRAITS[id];
  if (portrait) return <img className={cx(styles.portrait, className)} src={portrait.src} alt={portrait.alt} width={120} height={140} />;
  const a = avatarOf(id);
  return (
    <svg className={className} viewBox="0 0 120 140" role="img" aria-label="Stylised avatar">
      <g transform={`rotate(${a.tilt.toFixed(1)} 60 70)`}>
        <g fill={BONE} stroke={SHADE} strokeWidth="2">
          {HEADS[a.head]}
        </g>
        {mark(a)}
        <path d="M44 76q16 10 32 0" fill="none" stroke={SHADE} strokeWidth="2" opacity=".6" />
        {eyes(a)}
        <path d="M44 92h32" stroke={INK} strokeWidth="3" />
        <path d="M50 88v8M56 88v8M62 88v8M68 88v8" stroke={INK} strokeWidth="2" />
        {gear(a)}
        <path d="M20 140c4-24 20-34 40-34s36 10 40 34z" fill={a.deep} stroke={a.accent} strokeWidth="2.5" />
      </g>
    </svg>
  );
}

/**
 * A character's face in the 120x140 tile. Malakor is the winner's crop of his full figure
 * (`translate(-6 -2) scale(.33)` inside the tile), rebuilt as a clipped box with the same
 * geometry: offsets -6/120 and -2/140, width 132/120.
 */
export function CharacterFace({ id, className }: { readonly id: string; readonly className?: string | undefined }) {
  if (id === ART_CHARACTER) {
    return (
      <span className={cx(styles.box, className)}>
        <span className={styles.tile}>
          <span className={styles.crop}>
            <MalakorArt />
          </span>
        </span>
      </span>
    );
  }
  return <GenericAvatar id={id} className={cx(styles.svg, className)} />;
}
