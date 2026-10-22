// The Voice tab in jsdom against a scripted daemon that keeps the override in memory, with a fake
// AudioContext: the effective choice, save and reset, the key check, a preview played and one
// refused by the gate, the bench played line by line, every async state, no key field anywhere
// and no enum id on screen.
import { act, cleanup, fireEvent, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ProviderStatus, VoiceBench, VoiceChoice, VoicePreview, VoiceSettings } from "@shadowrunr/contract";
import { fail, mountPage, offline, ok, role, roles, settle, type Script } from "../../kit/testing";
import { Layer2 } from "./Layer2";
import { malakorDetail } from "./testgen";
import { buildChoice, fieldsOf } from "./voiceModel";
import { decodePcm16 } from "./voiceAudio";

let dispose: (() => void) | undefined;
afterEach(() => {
  cleanup();
  dispose?.();
  played.length = 0;
});

/** What the fake AudioContext was asked to play: samples and rate, in order. */
const played: { samples: number; rate: number }[] = [];

class FakeSource {
  buffer: { length: number; sampleRate: number } | null = null;
  onended: (() => void) | null = null;
  connect() {}
  start() {
    played.push({ samples: this.buffer?.length ?? 0, rate: this.buffer?.sampleRate ?? 0 });
    setTimeout(() => this.onended?.(), 0);
  }
  stop() {
    this.onended?.();
  }
}
class FakeAudioContext {
  destination = {};
  createBuffer(_channels: number, length: number, sampleRate: number) {
    return { length, sampleRate, copyToChannel() {} };
  }
  createBufferSource() {
    return new FakeSource();
  }
  resume() {
    return Promise.resolve();
  }
}
beforeAll(() => {
  vi.stubGlobal("AudioContext", FakeAudioContext);
});

/** Base64 PCM16 of `n` samples. */
const pcm = (n: number): string => btoa("\u0000@".repeat(n));

const PACKAGE: VoiceChoice = { provider: "kokoro", kokoro: { voice: "malakor-a", sid: 52, fx: "light" }, elevenlabs: { model: "eleven_v4_turbo" } };
const SAVED: VoiceChoice = { provider: "elevenlabs", kokoro: { speed: 1.1 }, elevenlabs: { voice_id: "43UFEkkGDp1aWG45TVp0" } };
const KOKORO: ProviderStatus = { provider: "kokoro", ready: true, key: "not_needed" };
const ELEVEN: ProviderStatus = { provider: "elevenlabs", ready: false, key: "set", reason: "Not verified yet." };

interface Daemon {
  override: VoiceChoice | undefined;
  providers: ProviderStatus[];
  sent: { method: string; raw: string }[];
  settings?: () => ReturnType<Script>;
  preview?: (raw: string) => ReturnType<Script>;
  bench?: () => ReturnType<Script>;
}

function settingsOf(d: Daemon): VoiceSettings {
  const o = d.override;
  const effective: VoiceChoice = o ? { ...PACKAGE, ...o, kokoro: { ...PACKAGE.kokoro, ...o.kokoro }, elevenlabs: { ...PACKAGE.elevenlabs, ...o.elevenlabs } } : PACKAGE;
  return {
    character: "malakor",
    effective,
    package: PACKAGE,
    ...(o ? { override: o } : {}),
    providers: d.providers,
    label: effective.provider === "elevenlabs" ? "ElevenLabs Malakor (crypt king alt1), turbo" : `Kokoro ${effective.kokoro?.voice ?? ""} (sid 52), fx light`,
  };
}

function daemon(o: Partial<Daemon> = {}): Daemon & { script: Script } {
  const d: Daemon = { override: undefined, providers: [KOKORO, ELEVEN], sent: [], ...o };
  const script: Script = (method, raw) => {
    d.sent.push({ method, raw });
    switch (method) {
      case "character.get":
        return ok(malakorDetail());
      case "voice.settings.get":
        return d.settings ? d.settings() : ok(settingsOf(d));
      case "voice.settings.set": {
        // The one override the save test writes; the request itself is asserted as sent.
        d.override = raw.includes('"choice":null') ? undefined : SAVED;
        return ok(settingsOf(d));
      }
      case "voice.verify": {
        const s: ProviderStatus = { provider: "elevenlabs", ready: true, key: "verified", checked_ms: Date.UTC(2026, 9, 22, 12) };
        d.providers = [KOKORO, s];
        return ok(s);
      }
      case "voice.preview": {
        if (d.preview) return d.preview(raw);
        const v: VoicePreview = { provider: "kokoro", label: "Kokoro malakor-a (sid 52), fx light", sample_rate: 24000, pcm: pcm(12), duration_ms: 2400, chars: 31, visemes: true };
        return ok(v);
      }
      case "voice.bench": {
        if (d.bench) return d.bench();
        const b: VoiceBench = {
          provider: "kokoro",
          label: "Kokoro malakor-a (sid 52), fx light",
          lines: [
            { id: "menace-1", text: "Kneel.", delivery: { emotion: "menace", intensity: 0.8, pace: "slow" }, sample_rate: 24000, pcm: pcm(4), duration_ms: 900, chars: 6 },
            { id: "glee-1", text: "Delicious.", delivery: { emotion: "glee", intensity: 0.6, pace: "fast" }, sample_rate: 22050, pcm: pcm(7), duration_ms: 1200, chars: 10 },
            { id: "sarcasm-1", text: "Oh, wonderful.", delivery: { emotion: "sarcasm", intensity: 0.5, pace: "normal" }, sample_rate: 24000, pcm: "", duration_ms: 0, chars: 0, error: "the sidecar exited" },
          ],
        };
        return ok(b);
      }
      default:
        return fail("method_not_found", method);
    }
  };
  return Object.assign(d, { script });
}

async function mount(script: Script) {
  const sub = ["malakor", "package", "voice"];
  const m = mountPage(<Layer2 sub={sub} />, script, `#/characters/${sub.join("/")}`);
  dispose = m.dispose;
  await settle(8);
  return m;
}

const click = (el: Element | null | undefined) => act(async () => fireEvent.click(el ?? document.body));
const type = (r: string, value: string) => act(async () => fireEvent.change(role(r) ?? document.body, { target: { value } }));
/** The params of each call to `method`, as the JSON the console sent. */
const sentOf = (d: Daemon, method: string) => d.sent.filter((s) => s.method === method).map((s) => s.raw.slice(s.raw.indexOf('"params":') + 9, -1));
const input = (r: string) => role(r) as HTMLInputElement;

describe("characters/<id>/package/voice", () => {
  it("shows the effective choice, the package as placeholders and the override as values", async () => {
    const d = daemon({ override: { provider: "kokoro", kokoro: { voice: "malakor-b" } } });
    await mount(d.script);
    expect(role("voice-effective")?.textContent).toContain("Kokoro malakor-b (sid 52), fx light");
    expect(role("voice-effective")?.textContent).toContain("local override");
    expect(input("voice-kokoro-voice").value).toBe("malakor-b");
    expect(input("voice-kokoro-voice").placeholder).toBe("package: malakor-a");
    expect(input("voice-kokoro-sid").value).toBe("");
    expect(input("voice-kokoro-sid").placeholder).toBe("package: 52");
    expect((role("voice-kokoro-fx") as HTMLSelectElement).selectedOptions[0]?.textContent).toBe("Package: Light");
    expect(role("voice-choice")?.textContent).toContain("Takes effect at the next stream start.");
  });

  it("saves the override and resets it to the package default with null", async () => {
    const d = daemon();
    await mount(d.script);
    expect(role("voice-effective")?.textContent).toContain("package default");
    expect((role("voice-reset") as HTMLButtonElement).disabled).toBe(true);
    await type("voice-kokoro-speed", "3");
    expect((role("voice-save") as HTMLButtonElement).disabled).toBe(true);
    expect(role("voice-choice")?.textContent).toContain("Kokoro speed is a multiplier from 0.5 to 2.");
    await type("voice-kokoro-speed", "1.1");
    await click(role("voice-choice")?.querySelector("button[aria-pressed='false']"));
    await type("voice-eleven-voice", "43UFEkkGDp1aWG45TVp0");
    await click(role("voice-save"));
    await settle();
    expect(sentOf(d, "voice.settings.set")).toEqual([
      JSON.stringify({ character: "malakor", choice: SAVED }),
    ]);
    expect(role("voice-choice")?.textContent).toContain("Saved");
    expect(role("voice-effective")?.textContent).toContain("ElevenLabs Malakor (crypt king alt1)");
    expect(input("voice-eleven-voice").value).toBe("43UFEkkGDp1aWG45TVp0");
    await click(role("voice-reset"));
    await settle();
    expect(sentOf(d, "voice.settings.set")[1]).toBe(JSON.stringify({ character: "malakor", choice: null }));
    expect(role("voice-choice")?.textContent).toContain("Back to the package default");
    expect(role("voice-effective")?.textContent).toContain("package default");
    expect(input("voice-kokoro-speed").value).toBe("");
  });

  it("checks the key and shows the provider's new status", async () => {
    const d = daemon();
    await mount(d.script);
    const rows = roles("voice-status-row");
    expect(rows.map((r) => r.getAttribute("data-ready"))).toEqual(["true", "false"]);
    expect(rows[0]?.textContent).toContain("No key needed");
    expect(rows[0]?.querySelector("[data-role='voice-verify']")).toBeNull();
    expect(rows[1]?.textContent).toContain("Key set, not verified");
    expect(rows[1]?.textContent).toContain("Not verified yet.");
    await click(rows[1]?.querySelector("[data-role='voice-verify']"));
    await settle();
    expect(sentOf(d, "voice.verify")).toEqual([JSON.stringify({ provider: "elevenlabs" })]);
    const eleven = roles("voice-status-row")[1];
    expect(eleven?.getAttribute("data-ready")).toBe("true");
    expect(eleven?.textContent).toContain("Key verified");
    expect(eleven?.textContent).toContain("checked");
    expect(role("voice-status")?.textContent).toContain("ELEVENLABS_API_KEY");
  });

  it("speaks a preview with its delivery and plays it, and shows a gate refusal", async () => {
    let refuse = false;
    const d = daemon({ preview: () => (refuse ? fail("invalid_params", "the line names a real person") : ok({ provider: "elevenlabs", label: "ElevenLabs Malakor (crypt king alt1), turbo", sample_rate: 44100, pcm: pcm(10), duration_ms: 2400, chars: 31, visemes: false })) });
    await mount(d.script);
    expect((role("voice-preview-speak") as HTMLButtonElement).disabled).toBe(true);
    await type("voice-preview-text", "Kneel, mortals. The stream begins.");
    await type("voice-preview-emotion", "menace");
    await type("voice-preview-intensity", "0.8");
    await type("voice-preview-pace", "slow");
    await click(role("voice-preview-speak"));
    await settle();
    expect(sentOf(d, "voice.preview")).toEqual([JSON.stringify({ character: "malakor", text: "Kneel, mortals. The stream begins.", delivery: { emotion: "menace", intensity: 0.8, pace: "slow" } })]);
    expect(played).toEqual([{ samples: 10, rate: 44100 }]);
    const res = role("voice-preview-result")?.textContent ?? "";
    expect(res).toContain("ElevenLabs");
    expect(res).toContain("2.4 s · 31 chars · mouth by loudness");
    refuse = true;
    await click(role("voice-preview-speak"));
    await settle();
    expect(role("voice-preview")?.textContent).toContain("The output gate refused this line");
    expect(role("voice-preview")?.textContent).toContain("the line names a real person");
    expect(role("voice-preview-result")).toBeNull();
    expect(played).toHaveLength(1);
  });

  it("renders the bench, plays one line and all in order, and shows a failed line's error", async () => {
    const d = daemon();
    await mount(d.script);
    await click(role("voice-bench-render"));
    await settle();
    const lines = roles("voice-bench-line");
    expect(lines.map((l) => l.getAttribute("data-line-id"))).toEqual(["menace-1", "glee-1", "sarcasm-1"]);
    expect(lines[0]?.textContent).toContain("Menace 80% · Slow pace");
    expect(lines[0]?.textContent).toContain("0.9 s · 6 chars");
    expect(lines[2]?.getAttribute("data-failed")).toBe("true");
    expect(lines[2]?.textContent).toContain("the sidecar exited");
    expect(within(lines[2] as HTMLElement).queryByRole("button")).toBeNull();
    await click(within(lines[1] as HTMLElement).getByRole("button", { name: "Play glee-1" }));
    await settle();
    expect(played).toEqual([{ samples: 7, rate: 22050 }]);
    played.length = 0;
    await click(role("voice-bench-play-all"));
    await settle(8);
    expect(played).toEqual([
      { samples: 4, rate: 24000 },
      { samples: 7, rate: 22050 },
    ]);
  });

  it("says how to add a bench when the package has none", async () => {
    await mount(daemon({ bench: () => fail("not_found", "no voice/bench.toml") }).script);
    await click(role("voice-bench-render"));
    await settle();
    expect(role("voice-bench")?.textContent).toContain("Add voice/bench.toml to the package");
    cleanup();
    dispose?.();
    await mount(daemon({ bench: () => ok({ provider: "kokoro", label: "Kokoro", lines: [] }) }).script);
    await click(role("voice-bench-render"));
    await settle();
    expect(role("voice-bench")?.querySelector("[data-state='empty']")).not.toBeNull();
  });

  it("shows loading, the daemon offline, its voice service refusing and an error", async () => {
    const m = mountPage(<Layer2 sub={["malakor", "package", "voice"]} />, (method) => (method === "character.get" ? ok(malakorDetail()) : { hang: true }), "#/characters/malakor/package/voice");
    dispose = m.dispose;
    await settle(8);
    expect(role("characters-voice-tab")?.querySelector("[data-state='loading']")).not.toBeNull();
    cleanup();
    dispose?.();
    await mount((method, raw) => (method === "character.get" ? ok(malakorDetail()) : offline(method, raw)));
    expect(role("characters-voice-tab")?.querySelector("[data-state='offline']")).not.toBeNull();
    cleanup();
    dispose?.();
    await mount(daemon({ settings: () => fail("unavailable", "voice.settings.get is not implemented yet") }).script);
    const tab = role("characters-voice-tab");
    expect(tab?.querySelector("[data-state='degraded']")?.textContent).toContain("The daemon's voice service cannot answer");
    expect(tab?.textContent).toContain("voice.settings.get is not implemented yet");
    cleanup();
    dispose?.();
    await mount(daemon({ settings: () => fail("io", "voice.json is not valid JSON") }).script);
    expect(role("characters-voice-tab")?.textContent).toContain("voice.json is not valid JSON");
  });

  it("has no field for the key anywhere and renders no enum id", async () => {
    await mount(daemon().script);
    await click(role("voice-bench-render"));
    await settle();
    expect(document.querySelector("input[type='password']")).toBeNull();
    for (const f of document.querySelectorAll("input, textarea, select")) {
      expect(f.getAttribute("name") ?? "").not.toMatch(/key/i);
      expect(f.closest("label")?.textContent ?? "").not.toMatch(/key/i);
    }
    await click(role("voice-choice")?.querySelector("button[aria-pressed='false']"));
    expect(document.querySelector("input[type='password']")).toBeNull();
    for (const f of document.querySelectorAll("input, textarea, select")) expect(f.closest("label")?.textContent ?? "").not.toMatch(/key/i);
    const text = role("characters-voice-tab")?.textContent ?? "";
    for (const id of ["not_needed", "elevenlabs", "kokoro", "menace", "sarcasm", "slow", "normal"]) expect(text).not.toContain(id);
  });
});

describe("the voice form's model", () => {
  it("round-trips a choice and leaves out an empty table", () => {
    const c: VoiceChoice = { provider: "kokoro", kokoro: { voice: "malakor-a", sid: 52, fx: "light", speed: 1.05 } };
    expect(buildChoice("kokoro", fieldsOf(c))).toEqual({ ok: true, choice: c });
    expect(buildChoice("elevenlabs", fieldsOf(undefined))).toEqual({ ok: true, choice: { provider: "elevenlabs" } });
    expect(buildChoice("kokoro", { ...fieldsOf(undefined), sid: "1.5" }).ok).toBe(false);
    expect(buildChoice("elevenlabs", { ...fieldsOf(undefined), elevenSpeed: "1.5" }).ok).toBe(false);
  });

  it("decodes PCM16 little-endian", () => {
    expect([...decodePcm16(btoa("\u0000@\u0000À"))]).toEqual([0.5, -0.5]);
  });
});
