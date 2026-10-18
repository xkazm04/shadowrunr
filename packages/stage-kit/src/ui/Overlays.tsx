import type { Case } from "@shadowrunr/contract";
import { DISCLOSURE, SEGMENT_TITLE } from "../lore";
import type { SceneState } from "../sceneModel";

export const TRANSITION_MS = 1900;
export const INTRO_MS = 4600;
export const MOMENT_FLASH_MS = 900;
export const MOMENT_CHIP_MS = 3600;
export const SEAL_MS = 4200;

const LAYOUT_TITLE = {
  boot: "The Spire wakes",
  improv: SEGMENT_TITLE["SEG-01"],
  hexmap: SEGMENT_TITLE["SEG-02A"],
  descoped: "A descoped segment",
} as const;

/** Always on screen, above everything (contract with the audience, not a decoration). */
export function DisclosureBadge() {
  return (
    <div className="disclosure" role="note">
      <span className="disclosure-dot" />
      {DISCLOSURE}
    </div>
  );
}

export function TopBar({
  scene,
  caseFile,
  sourceTag,
}: {
  scene: SceneState;
  caseFile: Case | null;
  sourceTag: string;
}) {
  const seg = scene.segment;
  const hexmap = scene.layout === "hexmap";
  return (
    <div className="topbar">
      <div className="brand">
        <svg viewBox="0 0 40 40" width="38" height="38" className="brand-mark">
          <path d="M20 2 L24 16 L20 38 L16 16 Z" fill="#0f1312" stroke="#5fe0bf" strokeWidth="1.5" />
          <path d="M8 14 L12 22 L10 34 M32 14 L28 22 L30 34" stroke="#c9a45a" strokeWidth="1.5" fill="none" />
        </svg>
        <div>
          <div className="brand-name">THE OBSIDIAN SPIRE</div>
          <div className="brand-seg">{seg ? `${seg} · ${SEGMENT_TITLE[seg]}` : "awaiting the first segment"}</div>
        </div>
      </div>
      {hexmap && (
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
        <span className="source-pill">{sourceTag}</span>
      </div>
    </div>
  );
}

export function TransitionWipe({ scene, now }: { scene: SceneState; now: number }) {
  const t = scene.transition;
  if (!t || t.from === "boot") return null;
  const age = now - t.at;
  if (age < 0 || age > TRANSITION_MS) return null;
  return (
    <div className="wipe" key={t.id}>
      <div className="wipe-band">
        <div className="wipe-kicker">{t.cause === "handover" ? "HANDOVER" : "NEW SEGMENT"}</div>
        <div className="wipe-title">{LAYOUT_TITLE[t.to]}</div>
        {t.reason && <div className="wipe-reason">{t.reason}</div>}
      </div>
    </div>
  );
}

export function Intro({ scene, now }: { scene: SceneState; now: number }) {
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
          {m.character === "malakor" ? "Arch-Lich Malakor" : m.character} · tone: {m.toneVariant}
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

export function SealToast({ scene, now }: { scene: SceneState; now: number }) {
  const s = scene.seals[scene.seals.length - 1];
  if (!s) return null;
  const age = now - s.at;
  if (age < 0 || age > SEAL_MS) return null;
  return (
    <div className="seal-toast" key={s.at}>
      <span className="seal-seg">{s.segment}</span> sealed
      {s.ses !== undefined && (
        <span className="seal-ses">
          SES {s.ses >= 0 ? "+" : ""}
          {s.ses.toFixed(2)}
        </span>
      )}
    </div>
  );
}

export function Outro({ scene, now }: { scene: SceneState; now: number }) {
  const o = scene.outro;
  if (!o || now < o.at) return null;
  const minions = new Set([...scene.chats.values()].map((m) => m.viewer.id)).size;
  const stats: [string, number][] = [
    ["minions in the ledger", minions],
    ["hexes burned through", scene.revealOrder.length],
    ["clips marked", scene.moments.length],
    ["lines redacted by the Lord", scene.redactions.length],
  ];
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
