// The 1920x1080 broadcast canvas. Every element sits in a named region; the active layout
// decides each region's rect, and CSS transitions animate the move on segment and screen changes.
//
// The face is drawn at one size (FACE_PX) and scaled to its region with a CSS transform, so a
// handover glides it from the host room's big box into the corner on the compositor: the canvas
// never re-allocates mid-move, and the transform runs on the same 0.95 s curve as the region.

import { useEffect, useMemo, type CSSProperties, type ReactNode } from "react";

import type { Case } from "@shadowrunr/contract";
import { captionAt, isContentScreen, type MapModel, type SceneState } from "@shadowrunr/stage-kit";
import {
  BrowserWindow,
  CaptionPanel,
  ChatLane,
  Decrees,
  DisclosureBadge,
  EndCard,
  ExplainerCard,
  FoilCard,
  GradeTallyPanel,
  HexInspector,
  Hexmap,
  Intro,
  JobTicker,
  MomentMarks,
  ObserverBadge,
  Outro,
  SealToast,
  Silhouette,
  TopBar,
  TransitionWipe,
  VotePanel,
  visiblePoll,
} from "@shadowrunr/stage-kit/ui";
import { Avatar } from "./avatar/Avatar";
import { lookBox, lookFor } from "./avatar/look";
import type { FaceSource } from "./avatar/types";
import type { FaceVariant } from "./avatar/variants";
import { FACE_PX, hexFrame, layoutRects, type Rect, type Region } from "./layouts";
import { StandingCard, VoiceCaptionPanel, cardShowing, tickerActive } from "./OnAir";
import type { VoiceCaptionView } from "./voice/captions";
import { SYNTHETIC_VOICE } from "./voice/names";
import type { FaceState } from "./voice/report";

function RegionBox({ name, rect, children }: { name: Region; rect: Rect; children: ReactNode }) {
  const style: CSSProperties = { left: rect.x, top: rect.y, width: rect.w, height: rect.h };
  return (
    <div className={`region region-${name}${rect.hidden ? " hidden" : ""}`} style={style} aria-hidden={rect.hidden || undefined}>
      {children}
    </div>
  );
}

function isSpeaking(caption: ReturnType<typeof captionAt>, now: number): boolean {
  if (!caption) return false;
  const e = now - caption.at;
  return e >= 0 && e <= caption.spokenMs + 300;
}

export function Stage({
  scene,
  now,
  map,
  caseFile,
  sourceTag,
  debug,
  broadcast = false,
  voice = false,
  clock,
  overlay,
  face,
  rigBase = null,
  variant = "glow",
  onCaptionSpeaking,
  voiceCaption,
  onFace,
}: {
  scene: SceneState;
  now: number;
  map: MapModel;
  caseFile: Case | null;
  sourceTag: string;
  debug: boolean;
  /** The OBS view: no production notes on the canvas. */
  broadcast?: boolean;
  /** A synthetic voice is configured (the disclosure says so). */
  voice?: boolean;
  /** The stream clock for the top bar (live mode). */
  clock?: string;
  /** Feed-health slates, drawn above the scene and below the disclosure. */
  overlay?: ReactNode;
  /** Malakor's face signal; without one the slot shows the silhouette only. */
  face?: FaceSource;
  /** The daemon's `/avatar/` base, when there is a daemon (else the face is the placeholder). */
  rigBase?: string | null;
  /** Which renderer draws the face (`?avatar=`). */
  variant?: FaceVariant;
  /** Hears whether a `say` caption is on screen (the face's level-less speaking phase). */
  onCaptionSpeaking?: (speaking: boolean) => void;
  /** Captions follow the voice's audio: the caption it gives now (absent: the estimated timing). */
  voiceCaption?: VoiceCaptionView | null;
  /** Hears which face the avatar slot draws. */
  onFace?: (state: FaceState) => void;
}) {
  const poll = visiblePoll(scene, now);
  const rects = layoutRects(scene.layout, { poll: poll !== null, foil: scene.foil !== null });
  if (!tickerActive(scene, now)) rects.ticker = { ...rects.ticker, hidden: true };
  // The streamer layout: Malakor cut out in the corner, the content behind him.
  const corner = isContentScreen(scene.layout);
  // Captions queue under a burst: the one showing now, not simply the latest line said.
  const caption = captionAt(scene, now);
  const speaking = voiceCaption === undefined ? isSpeaking(caption, now) : (voiceCaption?.speaking ?? false);
  useEffect(() => onCaptionSpeaking?.(speaking), [speaking, onCaptionSpeaking]);
  const faceScale = Math.min(rects.avatar.w, rects.avatar.h) / FACE_PX;
  const look = useMemo(() => lookBox(), []);
  const gaze = lookFor(scene.layout, speaking && caption ? caption.replyingTo : null);
  useEffect(() => look.set(gaze), [look, gaze]);
  const frame = hexFrame(scene.layout, rects);
  const record = scene.activeRecord === null ? undefined : scene.records[scene.activeRecord];
  const content = scene.layout === "browser" || scene.layout === "desk";

  return (
    <div className={`canvas layout-${scene.layout}${corner ? " layout-corner" : ""}${debug ? " debug" : ""}`} data-layout={scene.layout}>
      <div className="backdrop">
        <div className="backdrop-grid" />
        <div className="backdrop-glow" />
      </div>

      <RegionBox name="topbar" rect={rects.topbar}>
        <TopBar scene={scene} caseFile={caseFile} sourceTag={sourceTag} onAir={broadcast} {...(clock === undefined ? {} : { clock })} />
      </RegionBox>

      <RegionBox name="hexmap" rect={rects.hexmap}>
        <Hexmap
          map={map}
          scene={scene}
          now={now}
          caseFile={caseFile}
          {...(frame ? { frame } : {})}
          highlight={scene.layout === "desk" && record ? record.hex : null}
        />
      </RegionBox>

      <RegionBox name="browser" rect={rects.browser}>
        <BrowserWindow scene={scene} map={map} onAir={broadcast} />
      </RegionBox>

      <RegionBox name="avatar" rect={rects.avatar}>
        <div className={`avatar-slot${corner ? " cut-out" : ""}`}>
          <Silhouette speaking={speaking} small={corner} />
          {face && (
            <div className="avatar-stand" style={{ width: FACE_PX, height: FACE_PX, transform: `translate(-50%, -50%) scale(${faceScale.toFixed(4)})` }}>
              <Avatar className="avatar-face" rigBase={rigBase} size={FACE_PX} source={face} variant={variant} look={look} {...(onFace ? { onFace } : {})} />
            </div>
          )}
          {voice && <div className="voice-label">{SYNTHETIC_VOICE}</div>}
          {!corner && !broadcast && <div className="avatar-note">avatar slot · face rig or procedural placeholder</div>}
        </div>
      </RegionBox>

      <RegionBox name="caption" rect={rects.caption}>
        {voiceCaption === undefined ? (
          <CaptionPanel caption={caption} redactions={scene.redactions} now={now} compact={corner} onAir={broadcast} />
        ) : (
          <VoiceCaptionPanel view={voiceCaption} redactions={scene.redactions} now={now} compact={corner} onAir={broadcast} />
        )}
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
        <ChatLane scene={scene} now={now} title={scene.segment === "SEG-02A" ? "Conscripted auditors" : "The minions"} />
      </RegionBox>

      <RegionBox name="card" rect={rects.card}>
        {cardShowing(scene, now) ? (
          <ExplainerCard card={scene.card} now={now} />
        ) : content ? null : (
          <StandingCard scene={scene} poll={poll !== null && poll.closedAt === undefined} />
        )}
      </RegionBox>

      <RegionBox name="decrees" rect={rects.decrees}>
        <Decrees scene={scene} now={now} onAir={broadcast} />
      </RegionBox>

      <RegionBox name="ticker" rect={rects.ticker}>
        <JobTicker scene={scene} now={now} onAir={broadcast} />
      </RegionBox>

      <MomentMarks scene={scene} now={now} />
      <SealToast scene={scene} now={now} onAir={broadcast} />
      <TransitionWipe scene={scene} now={now} />
      <Intro scene={scene} now={now} onAir={broadcast} />
      <Outro scene={scene} now={now} />
      <EndCard scene={scene} now={now} />
      {overlay}
      <DisclosureBadge voice={voice} />
    </div>
  );
}
