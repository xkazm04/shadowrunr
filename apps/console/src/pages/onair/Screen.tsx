// The stream screen (the winner's stage.js `P3.Stage`), rebuilt as components over a stage-kit
// scene: a 16:9 screenprint of the broadcast. Its hotspots are route links into layer 2, and
// every fidelity element carries a stable `data-role="onair-…"` for the capture check.
import type { SegmentId } from "@shadowrunr/contract";
import { FoilArt, MalakorArt } from "@shadowrunr/stage-kit/ui";
import type { CSSProperties } from "react";
import { RouteLink, pageRoute, type Route } from "../../router";
import type { FeedSnapshot } from "./player";
import styles from "./Screen.module.css";
import { ScreenHex } from "./ScreenHex";
import { CaptionText, ChatLines, DecreeSeal, ScreenSet, VoteRows, lifeStyle } from "./ScreenParts";
import {
  captionView,
  cardView,
  chatRows,
  decreeView,
  foilView,
  gradesOf,
  momentView,
  segmentOf,
  slateView,
  sourceLabel,
  voteView,
} from "./view";

/** The rundown pills: the two live segments, then a count of the descoped ones. */
export interface RundownPill {
  readonly id: SegmentId;
  readonly title: string;
}

export interface ScreenProps {
  readonly snapshot: FeedSnapshot;
  readonly pills: readonly RundownPill[];
  readonly descoped: number;
  /** Where the chat hotspot leads: the transcript of the run on screen. */
  readonly chatRoute: Route;
  /** Overrides the source line (offline, loading, no runs). */
  readonly status?: string | undefined;
  /** The off-air slate for designed states (offline, empty), over the sign-off one. */
  readonly offAir?: string | undefined;
  /** The Lord's dispatch, when the latest plan is known. */
  readonly dispatch?: string | undefined;
}

export function Screen(props: ScreenProps) {
  const { scene, position, speed, playing, live } = props.snapshot;
  const seg = segmentOf(scene);
  const caption = captionView(scene, position, speed);
  const vote = voteView(scene, position, speed);
  const card = cardView(scene, position, speed);
  const decree = decreeView(scene, position, speed);
  const moment = momentView(scene, position, speed);
  const foil = foilView(scene);
  const slate = props.offAir ?? slateView(scene, live);
  const talking = caption !== null && caption.talk.live;
  const rootStyle = {
    ...(caption ? { "--talk-iter": String(Math.max(1, Math.round(caption.talkSecs / 0.34))), "--talk-d": `${caption.talk.delayMs}ms` } : {}),
    ...(vote?.closing ? { "--vote-gone": `${5000 + vote.closing.delayMs}ms` } : {}),
  } as CSSProperties;

  return (
    <div
      className={styles.screen}
      data-role="onair-screen"
      data-seg={seg}
      data-poll={vote ? true : undefined}
      data-closing={vote?.closing ? true : undefined}
      data-foil={foil ? true : undefined}
      data-talk={talking ? caption.parity : undefined}
      data-slip={props.dispatch ? true : undefined}
      data-slate={slate !== null ? true : undefined}
      data-paused={!playing && !live ? true : undefined}
      style={rootStyle}
    >
      <ScreenSet />
      <div className={styles.scrHud} data-role="onair-hud">
        <RouteLink to={pageRoute("onair", "rundown")} className={`${styles.rd} ${styles.hot}`} aria-label="The rundown: the night’s segments, handover clocks and what is descoped" data-role="onair-rundown">
          {props.pills.map((p) => (
            <span key={p.id} className={styles.pill} data-on={seg === p.id || undefined}>
              <b>{p.id}</b> {p.title}
            </span>
          ))}
          {props.descoped > 0 && <span className={`${styles.pill} ${styles.cut}`}>{props.descoped} more descoped</span>}
        </RouteLink>
        <div className={styles.scrLive}>
          <i />
          <span>{props.status ?? sourceLabel(scene, live, speed)}</span>
        </div>
      </div>

      <RouteLink to={pageRoute("characters")} className={`${styles.scrMal} ${styles.hot}`} aria-label="Arch-Lich Malakor: the character, her package and her tones" data-role="onair-malakor">
        <MalakorArt />
        <div className={styles.mkTag}>Stylised illustration</div>
        <div className={styles.scrFoil} aria-hidden="true" data-role="onair-foil">
          {foil && (
            <>
              <FoilArt id={foil.id} />
              <span>{foil.name}</span>
            </>
          )}
        </div>
      </RouteLink>

      <div className={styles.scrMid}>
        <div className={styles.scrRoom} data-role="onair-room">
          <div className={styles.rmT}>The Audience Chamber</div>
          <div className={styles.rmS}>SEG-01 · {props.pills.find((p) => p.id === "SEG-01")?.title ?? "Improvised Room"}</div>
        </div>
        <RouteLink to={pageRoute("segments", "tables", "SEG-02A")} className={`${styles.scrHex} ${styles.hot}`} aria-label="The investigation hexmap: segment tables, SEG-02A, grades DOC to SPC" data-role="onair-hex">
          <div className={styles.hxT}>
            <span>Case file</span>
            {scene.caseFile?.fictional !== false && <em className={styles.fict}>Fictional demo case</em>}
          </div>
          <ScreenHex caseFile={scene.caseFile} revealed={scene.revealOrder} grades={gradesOf(scene)} />
          <div className={styles.hxLeg} data-role="onair-grade-legend">
            <i data-g="DOC">DOC</i>
            <i data-g="REP">REP</i>
            <i data-g="CLM">CLM</i>
            <i data-g="SPC">SPC</i>
          </div>
        </RouteLink>
        <RouteLink to={pageRoute("onair", "dispatch")} className={`${styles.scrSlip} ${styles.hot}`} aria-label="The Lord’s dispatch: tonight’s orders, and what Malakor receives" data-role="onair-slip">
          <b>The Lord’s dispatch</b>
          <span className={styles.slT}>{props.dispatch ?? ""}</span>
        </RouteLink>
        {card && (
          <div key={card.key} className={styles.scrCard} style={lifeStyle(card.life)} data-role="onair-card">
            <b>{card.value.title}</b>
            <span>{card.value.body}</span>
          </div>
        )}
        {decree && (
          <div key={decree.key} className={styles.scrDecree} style={lifeStyle(decree.life)} data-role="onair-decree">
            <DecreeSeal />
            <div>
              <b>Decree · picked from chat</b>
              <span className={styles.dcT}>{decree.value}</span>
            </div>
          </div>
        )}
      </div>

      <div className={styles.scrRight}>
        <RouteLink to={pageRoute("segments", "ranks")} className={`${styles.scrVote} ${styles.hot}`} aria-label="Rank-weighted votes: vote weights are assumptions, rank thresholds and weights" data-role="onair-vote">
          <VoteRows vote={vote} />
        </RouteLink>
        <RouteLink to={props.chatRoute} className={`${styles.scrChat} ${styles.hot}`} aria-label="Chat and the picker: open the transcript viewer" data-role="onair-chat">
          <div className={styles.chH}>
            Chat <em>simulated viewers</em>
          </div>
          <ChatLines rows={chatRows(scene)} />
        </RouteLink>
      </div>

      <div className={styles.scrCap} data-role="onair-lower-third">
        <div className={styles.cpPlate}>
          <span className={styles.nm}>{caption?.name ?? "Arch-Lich Malakor"}</span>
          <span className={styles.rk}>{caption?.role ?? "Lord’s envoy"}</span>
          <span className={styles.ai}>AI character</span>
        </div>
        <CaptionText caption={caption} />
      </div>

      {moment && (
        <div key={moment.key} className={styles.scrMoment} style={lifeStyle(moment.life)} data-role="onair-moment">
          {moment.value}
        </div>
      )}
      <div className={styles.scrSlate} data-role="onair-slate">
        <b>{props.offAir !== undefined ? "Off air" : "Sign off"}</b>
        <span>{slate ?? ""}</span>
      </div>
    </div>
  );
}
