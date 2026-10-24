// SEG-03, the Table Read's two-shot: the guest's half (portrait, name plate, speaking glow), the
// name plate under the host, and the scene's title card. The host's half is the stage's own face.
//
// The portrait is the foil's flat SVG (art/FoilArt.tsx). A raster portrait can replace it with no
// code change: pass `src` (the OBS stage asks the daemon's `/avatar/` route for
// `guest-<foil>.png`, which it serves from the character's rig folder in <data>). The SVG stays
// underneath until the image has loaded, and stays alone when it fails, so a missing file costs one
// 404 and never a broken-image box.

import { useState } from "react";

import { foilInfo } from "../lore";
import type { SceneCard } from "../sceneModel";
import { FoilArt, hasFoilArt } from "../art/FoilArt";

/** How long a scene's title card plays large before it settles into the band (ms). */
export const SCENE_CARD_INTRO_MS = 4000;

/** The file a raster guest portrait is looked up under, beside the host's rig. */
export function guestPortraitFile(foil: string): string {
  return `guest-${foil}.png`;
}

function Initials({ name }: { name: string }) {
  const initials = name
    .split(" ")
    .filter((w) => w.toLowerCase() !== "the")
    .map((w) => w[0])
    .join("")
    .slice(0, 2);
  return (
    <svg className="foil-art guest-initials" viewBox="0 0 120 140" role="img" aria-label="Guest emblem">
      <circle cx="60" cy="70" r="46" fill="#101413" stroke="currentColor" strokeWidth="4" />
      <text x="60" y="84" textAnchor="middle" fontSize="40" fontWeight="700" fill="currentColor" fontFamily="Cinzel Variable, serif">
        {initials}
      </text>
    </svg>
  );
}

/** The guest's portrait: the SVG, and over it the raster `src` once it has loaded. */
export function GuestPortrait({ id, src }: { id: string; src?: string | null | undefined }) {
  const [img, setImg] = useState<{ src: string; state: "loading" | "ready" | "failed" } | null>(null);
  const current = src ? (img?.src === src ? img : { src, state: "loading" as const }) : null;
  return (
    <div className="guest-portrait" data-portrait={current?.state === "ready" ? "image" : "svg"}>
      {hasFoilArt(id) ? <FoilArt id={id} /> : <Initials name={foilInfo(id).name} />}
      {current && current.state !== "failed" && (
        <img
          className={`guest-image${current.state === "ready" ? " ready" : ""}`}
          src={current.src}
          alt=""
          onLoad={() => setImg({ src: current.src, state: "ready" })}
          onError={() => setImg({ src: current.src, state: "failed" })}
        />
      )}
    </div>
  );
}

/** The guest's half of the two-shot. `speaking`: its line is heard (or captioned) now. */
export function GuestShot({ guest, speaking, src }: { guest: string | null; speaking: boolean; src?: string | null | undefined }) {
  if (guest === null) {
    return (
      <div className="guest-shot empty" data-role="guest-shot">
        <div className="guest-plate">
          <span className="guest-name">The guest</span>
          <span className="guest-sub">arriving</span>
        </div>
      </div>
    );
  }
  const f = foilInfo(guest);
  return (
    <div className={`guest-shot${speaking ? " speaking" : ""}`} style={{ ["--foil" as string]: f.color }} data-role="guest-shot" data-guest={guest} data-speaking={speaking || undefined}>
      <div className="guest-glow" aria-hidden="true" />
      <GuestPortrait id={guest} src={src} />
      <div className="guest-plate">
        <span className="guest-name">{f.name}</span>
        <span className="guest-sub">original parody · not a real person</span>
      </div>
    </div>
  );
}

/** The host's name plate under his half of the two-shot. */
export function HostPlate({ name, speaking }: { name: string; speaking: boolean }) {
  return (
    <div className={`host-plate${speaking ? " speaking" : ""}`} data-role="host-plate">
      <span className="host-plate-name">{name}</span>
      <span className="host-plate-sub">your host</span>
    </div>
  );
}

/** The scene's title card: large for SCENE_CARD_INTRO_MS after `scene_start`, then a quiet band. */
export function SceneTitleCard({ card, now }: { card: SceneCard | null; now: number }) {
  if (!card) return null;
  const fresh = now - card.at < SCENE_CARD_INTRO_MS;
  const f = foilInfo(card.guest);
  return (
    <div className={`scene-card${fresh ? " fresh" : ""}`} key={card.id} style={{ ["--foil" as string]: f.color }} data-role="scene-card" data-scene={card.id}>
      <span className="scene-kicker">The Table Read</span>
      <span className="scene-title">{card.title}</span>
      <span className="scene-starring">
        starring <b>{f.name}</b>
      </span>
    </div>
  );
}
