// The 1920x1080 broadcast canvas. Every element sits in a named region; the active layout
// decides each region's rect, and CSS transitions animate the move on segment changes.

import type { CSSProperties, ReactNode } from "react";

import type { Case } from "./contract";
import { Silhouette } from "./art/Silhouette";
import type { MapModel } from "./hexgrid";
import { layoutRects, type Rect, type Region } from "./layouts";
import type { SceneState } from "./scene";
import { CaptionPanel } from "./ui/Caption";
import { ChatLane } from "./ui/ChatLane";
import { Decrees } from "./ui/Decrees";
import { ExplainerCard } from "./ui/ExplainerCard";
import { FoilCard, ObserverBadge } from "./ui/FoilCard";
import { Hexmap } from "./ui/Hexmap";
import { GradeTallyPanel, HexInspector } from "./ui/HexPanels";
import { JobTicker } from "./ui/JobTicker";
import { DisclosureBadge, Intro, MomentMarks, Outro, SealToast, TopBar, TransitionWipe } from "./ui/Overlays";
import { VotePanel, visiblePoll } from "./ui/VotePanel";
import { captionOpacity, wordTimings } from "./timing";

function RegionBox({ name, rect, children }: { name: Region; rect: Rect; children: ReactNode }) {
  const style: CSSProperties = { left: rect.x, top: rect.y, width: rect.w, height: rect.h };
  return (
    <div className={`region region-${name}${rect.hidden ? " hidden" : ""}`} style={style} aria-hidden={rect.hidden || undefined}>
      {children}
    </div>
  );
}

function isSpeaking(scene: SceneState, now: number): boolean {
  const c = scene.caption;
  if (!c) return false;
  const line = wordTimings(c.text);
  const e = now - c.at;
  return e >= 0 && e <= line.spokenMs + 300 && captionOpacity(line, e) > 0;
}

export function Stage({
  scene,
  now,
  map,
  caseFile,
  sourceTag,
  debug,
}: {
  scene: SceneState;
  now: number;
  map: MapModel;
  caseFile: Case | null;
  sourceTag: string;
  debug: boolean;
}) {
  const poll = visiblePoll(scene, now);
  const rects = layoutRects(scene.layout, { poll: poll !== null, foil: scene.foil !== null });
  const hexmap = scene.layout === "hexmap";
  const speaking = isSpeaking(scene, now);

  return (
    <div className={`canvas layout-${scene.layout}${debug ? " debug" : ""}`} data-layout={scene.layout}>
      <div className="backdrop">
        <div className="backdrop-grid" />
        <div className="backdrop-glow" />
      </div>

      <RegionBox name="topbar" rect={rects.topbar}>
        <TopBar scene={scene} caseFile={caseFile} sourceTag={sourceTag} />
      </RegionBox>

      <RegionBox name="hexmap" rect={rects.hexmap}>
        <Hexmap map={map} scene={scene} now={now} caseFile={caseFile} />
      </RegionBox>

      <RegionBox name="avatar" rect={rects.avatar}>
        <div className="avatar-slot">
          <Silhouette speaking={speaking} small={hexmap} />
          {!hexmap && <div className="avatar-note">avatar slot · placeholder silhouette</div>}
        </div>
      </RegionBox>

      <RegionBox name="caption" rect={rects.caption}>
        <CaptionPanel caption={scene.caption} redactions={scene.redactions} now={now} compact={hexmap} />
      </RegionBox>

      <RegionBox name="foil" rect={rects.foil}>
        <FoilCard scene={scene} now={now} />
      </RegionBox>

      <RegionBox name="observer" rect={rects.observer}>
        <ObserverBadge scene={scene} now={now} />
      </RegionBox>

      <RegionBox name="inspector" rect={rects.inspector}>
        <HexInspector map={map} scene={scene} now={now} />
      </RegionBox>

      <RegionBox name="tally" rect={rects.tally}>
        <GradeTallyPanel scene={scene} />
      </RegionBox>

      <RegionBox name="vote" rect={rects.vote}>
        {poll && <VotePanel poll={poll} now={now} />}
      </RegionBox>

      <RegionBox name="chat" rect={rects.chat}>
        <ChatLane scene={scene} now={now} title={hexmap ? "Conscripted auditors" : "The minions"} />
      </RegionBox>

      <RegionBox name="card" rect={rects.card}>
        <ExplainerCard card={scene.card} now={now} />
      </RegionBox>

      <RegionBox name="decrees" rect={rects.decrees}>
        <Decrees scene={scene} now={now} />
      </RegionBox>

      <RegionBox name="ticker" rect={rects.ticker}>
        <JobTicker scene={scene} now={now} />
      </RegionBox>

      <MomentMarks scene={scene} now={now} />
      <SealToast scene={scene} now={now} />
      <TransitionWipe scene={scene} now={now} />
      <Intro scene={scene} now={now} />
      <Outro scene={scene} now={now} />
      <DisclosureBadge />
    </div>
  );
}
