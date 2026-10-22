// The Voice tab's pure part: the override form's fields from a VoiceChoice and back (an empty
// field is no override, so the package default shows through), the speed bounds, and how a
// voice RPC's failure reads. The key never appears here: the daemon reads it from its env.
import type { ElevenlabsVoice, KokoroVoice, VoiceChoice, VoiceFx, VoiceProvider } from "@shadowrunr/contract";
import { viewOfError, type DataError, type ErrorView } from "../../data";
import { PROVIDER_NAME } from "./names";

/** The override form as typed: strings, "" meaning "no override, the package default". */
export interface VoiceFields {
  readonly voice: string;
  readonly sid: string;
  readonly fx: VoiceFx | "";
  readonly kokoroSpeed: string;
  readonly voiceId: string;
  readonly model: string;
  readonly elevenSpeed: string;
}

/** Speaking-rate bounds per provider: Kokoro's synth range, ElevenLabs' `voice_settings.speed`. */
export const SPEED_RANGE: Readonly<Record<VoiceProvider, readonly [number, number]>> = { kokoro: [0.5, 2], elevenlabs: [0.7, 1.2] };

/** The preview's text cap, as the daemon enforces it. */
export const PREVIEW_MAX = 400;

const str = (n: number | undefined): string => (n === undefined ? "" : String(n));

export function fieldsOf(c: VoiceChoice | undefined): VoiceFields {
  return {
    voice: c?.kokoro?.voice ?? "",
    sid: str(c?.kokoro?.sid),
    fx: c?.kokoro?.fx ?? "",
    kokoroSpeed: str(c?.kokoro?.speed),
    voiceId: c?.elevenlabs?.voice_id ?? "",
    model: c?.elevenlabs?.model ?? "",
    elevenSpeed: str(c?.elevenlabs?.speed),
  };
}

export type Built = { readonly ok: true; readonly choice: VoiceChoice } | { readonly ok: false; readonly error: string };

function speedOf(p: VoiceProvider, s: string): number | string | undefined {
  if (s.trim() === "") return undefined;
  const [lo, hi] = SPEED_RANGE[p];
  const n = Number(s);
  return Number.isFinite(n) && n >= lo && n <= hi ? n : `${PROVIDER_NAME[p]} speed is a multiplier from ${lo} to ${hi}.`;
}

/**
 * The override to save: the chosen provider and both providers' set fields (the other is kept so
 * switching back keeps its settings); a table with nothing set is left out.
 */
export function buildChoice(provider: VoiceProvider, f: VoiceFields): Built {
  const sid = f.sid.trim() === "" ? undefined : Number(f.sid);
  if (sid !== undefined && (!Number.isInteger(sid) || sid < 0)) return { ok: false, error: "Speaker id is a whole number, 0 or more." };
  const ks = speedOf("kokoro", f.kokoroSpeed);
  if (typeof ks === "string") return { ok: false, error: ks };
  const es = speedOf("elevenlabs", f.elevenSpeed);
  if (typeof es === "string") return { ok: false, error: es };
  const kokoro: KokoroVoice = {
    ...(f.voice.trim() === "" ? {} : { voice: f.voice.trim() }),
    ...(sid === undefined ? {} : { sid }),
    ...(f.fx === "" ? {} : { fx: f.fx }),
    ...(ks === undefined ? {} : { speed: ks }),
  };
  const elevenlabs: ElevenlabsVoice = {
    ...(f.voiceId.trim() === "" ? {} : { voice_id: f.voiceId.trim() }),
    ...(f.model.trim() === "" ? {} : { model: f.model.trim() }),
    ...(es === undefined ? {} : { speed: es }),
  };
  return {
    ok: true,
    choice: {
      provider,
      ...(Object.keys(kokoro).length === 0 ? {} : { kokoro }),
      ...(Object.keys(elevenlabs).length === 0 ? {} : { elevenlabs }),
    },
  };
}

/**
 * A voice RPC's failure as a page state. The daemon itself answers `unavailable` when its voice
 * service cannot serve (no transport reason): that is a degraded daemon, not an offline one.
 */
export function voiceError(e: DataError): ErrorView {
  const v = viewOfError(e);
  if (e.code !== "unavailable" || e.detail?.reason !== undefined) return v;
  return { ...v, state: "degraded", title: "The daemon's voice service cannot answer", hint: "The daemon is up; its voice service said why below." };
}

/** Seconds with one decimal: "2.4 s". */
export const secs = (ms: number): string => `${(ms / 1000).toFixed(1)} s`;
