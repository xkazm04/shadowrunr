import type { Case } from "@shadowrunr/contract";
import { SEGMENT_TITLE, disclosureText } from "../lore";
import { isContentScreen, type SceneState } from "../sceneModel";

export const TRANSITION_MS = 1900;
export const INTRO_MS = 4600;
export const MOMENT_FLASH_MS = 900;
export const MOMENT_CHIP_MS = 3600;
export const SEAL_MS = 4200;

/** Always on screen, above everything (contract with the audience, not a decoration).
 * `voice` is true only once a synthetic voice is configured; until then it reads "text only". */
export function DisclosureBadge({ voice = false }: { voice?: boolean }) {
  return (
    <div className="disclosure" role="note">
      <span className="disclosure-dot" />
      {disclosureText(voice)}
    </div>
  );
}

export function TopBar({
  scene,
  caseFile,
  sourceTag,
  clock,
  onAir = false,
}: {
  scene: SceneState;
  caseFile: Case | null;
  sourceTag: string;
  /** The stream's elapsed time (mm:ss), shown beside the source tag when given. */
  clock?: string;
  /** The broadcast view: the segment's title without its id. */
  onAir?: boolean;
}) {
  const seg = scene.segment;
  const caseShown = scene.segment === "SEG-02A" || isContentScreen(scene.layout);
  return (
    <div className="topbar">
      <div className="brand">
        <svg viewBox="0 0 40 40" width="38" height="38" className="brand-mark">
          <path d="M20 2 L24 16 L20 38 L16 16 Z" fill="#0f1312" stroke="#5fe0bf" strokeWidth="1.5" />
          <path d="M8 14 L12 22 L10 34 M32 14 L28 22 L30 34" stroke="#c9a45a" strokeWidth="1.5" fill="none" />
        </svg>
        <div>
          <div className="brand-name">THE OBSIDIAN SPIRE</div>
          <div className="brand-seg">{seg ? (onAir ? SEGMENT_TITLE[seg] : `${seg} · ${SEGMENT_TITLE[seg]}`) : "awaiting the first segment"}</div>
        </div>
      </div>
      {caseShown && (
        <div className="case-title">
          <span className="case-kicker">CASE FILE</span>
          <span className="case-name">{caseFile ? caseFile.title : "Unlabelled case (no case file loaded)"}</span>
          {caseFile?.fictional && <span className="fictional-tag">FICTIONAL</span>}
        </div>
      )}
      <div className="topbar-right">
        {scene.pendingSwitch && (
          <span className="pending-switch">⟶ {SEGMENT_TITLE[scene.pendingSwitch.to]}</span>
        )}
        {scene.meta && <span className="meta-pill">{scene.meta.audienceSize} minions</span>}
        {clock !== undefined && <span className="clock-pill">{clock}</span>}
        <span className="source-pill">{sourceTag}</span>
      </div>
    </div>
  );
}

export function TransitionWipe({ scene, now }: { scene: SceneState; now: number }) {
  const t = scene.transition;
  // A screen switch inside a segment moves the regions; only a segment change gets the wipe.
  if (!t || t.from === "boot" || t.cause === "screen" || t.segment === undefined) return null;
  const age = now - t.at;
  if (age < 0 || age > TRANSITION_MS) return null;
  return (
    <div className="wipe" key={t.id}>
      <div className="wipe-band">
        <div className="wipe-kicker">{t.cause === "handover" ? "HANDOVER" : "NEW SEGMENT"}</div>
        <div className="wipe-title">{SEGMENT_TITLE[t.segment]}</div>
        {t.reason && <div className="wipe-reason">{t.reason}</div>}
      </div>
    </div>
  );
}

export function Intro({ scene, now, onAir = false }: { scene: SceneState; now: number; onAir?: boolean }) {
  const m = scene.meta;
  if (!m) return null;
  const age = now - m.at;
  if (age < 0 || age > INTRO_MS) return null;
  return (
    <div className={`intro${INTRO_MS - age < 700 ? " leaving" : ""}`}>
      <div className="intro-seal">
        <div className="intro-kicker">NOW BROADCASTING FROM</div>
        <div className="intro-title">THE OBSIDIAN SPIRE</div>
        <div className="intro-sub">
          {m.character === "malakor" ? "Arch-Lich Malakor" : m.character}
          {!onAir && ` · tone: ${m.toneVariant}`}
        </div>
      </div>
    </div>
  );
}

export function MomentMarks({ scene, now }: { scene: SceneState; now: number }) {
  const last = scene.moments[scene.moments.length - 1];
  if (!last) return null;
  const age = now - last.at;
  if (age < 0 || age > MOMENT_CHIP_MS) return null;
  return (
    <>
      {age < MOMENT_FLASH_MS && <div className="moment-flash" key={`f${last.at}`} />}
      <div className="moment-chip" key={`c${last.at}`}>
        <span className="clip-icon">✦</span> CLIP MARKED · {last.label}
      </div>
    </>
  );
}

/** A segment sealed. On air it names the segment by its title and leaves out the evaluator's score. */
export function SealToast({ scene, now, onAir = false }: { scene: SceneState; now: number; onAir?: boolean }) {
  const s = scene.seals[scene.seals.length - 1];
  if (!s) return null;
  const age = now - s.at;
  if (age < 0 || age > SEAL_MS) return null;
  return (
    <div className="seal-toast" key={s.at}>
      <span className="seal-seg">{onAir ? SEGMENT_TITLE[s.segment] : s.segment}</span> sealed
      {!onAir && s.ses !== undefined && (
        <span className="seal-ses">
          SES {s.ses >= 0 ? "+" : ""}
          {s.ses.toFixed(2)}
        </span>
      )}
    </div>
  );
}

function ledgerStats(scene: SceneState): [string, number][] {
  const minions = new Set([...scene.chats.values()].map((m) => m.viewer.id)).size;
  return [
    ["minions in the ledger", minions],
    ["hexes burned through", scene.revealOrder.length],
    ["clips marked", scene.moments.length],
    ["lines redacted by the Lord", scene.redactions.length],
  ];
}

export function Outro({ scene, now }: { scene: SceneState; now: number }) {
  const o = scene.outro;
  if (!o || now < o.at) return null;
  const stats = ledgerStats(scene);
  return (
    <div className="outro">
      <div className="outro-card">
        <div className="outro-kicker">THE LEDGER IS SEALED</div>
        <div className="outro-reason">{o.reason}</div>
        <div className="outro-stats">
          {stats.map(([k, v]) => (
            <div key={k} className="outro-stat">
              <span className="outro-num">{v}</span>
              <span className="outro-key">{k}</span>
            </div>
          ))}
        </div>
        <div className="outro-catch">Let the deficit be recorded.</div>
        {scene.ended && <div className="outro-ended">transmission ended · {scene.ended.reason.replace(/_/g, " ")}</div>}
      </div>
    </div>
  );
}

/** The end card for a `stream_end` that came without a sign-off (the Outro covers one with). */
export function EndCard({ scene, now }: { scene: SceneState; now: number }) {
  const e = scene.ended;
  if (!e || scene.outro || now < e.at) return null;
  const stats = ledgerStats(scene);
  return (
    <div className="outro end-card">
      <div className="outro-card">
        <div className="outro-kicker">THE SPIRE GOES DARK</div>
        <div className="outro-reason">The transmission has ended.</div>
        <div className="outro-stats">
          {stats.map(([k, v]) => (
            <div key={k} className="outro-stat">
              <span className="outro-num">{v}</span>
              <span className="outro-key">{k}</span>
            </div>
          ))}
        </div>
        <div className="outro-ended">transmission ended · {e.reason.replace(/_/g, " ")}</div>
      </div>
    </div>
  );
}
