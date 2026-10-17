# 04 — Voice and character assets

Research date: **2026-10-17**. Scope: (A) what the owner already uses or has written down
about TTS and voice, (B) the current state of local expressive TTS and how to create a new
voice legally, (C) Leonardo.ai and Tripo for the character model, the 2D (Live2D) and 3D
(VRM) avatar routes, and the IP boundary against Mattel's Skeletor.

Target machine: Windows 11, RTX 4090 24 GB, 64 GB RAM (the same machine as the owner's
2026-09-29 TTS bake-off, which used a Ryzen 7 7800X3D).

**How claims are marked.** **[measured]** means it was measured on the owner's machine, from
the owner's records. **[vendor]** means the vendor's own figure. **UNVERIFIED** means it could
not be confirmed from a primary source on the research date. Web facts were gathered by
research agents on 2026-10-17 from the URLs cited. Pages that returned 403 are said to.

---

## TL;DR

1. **The owner already has a measured local voice stack.**
   - Kokoro-82M is the live preset voice, run through sherpa-onnx.
   - A four-engine cloning bake-off ran on this exact 4090: Qwen3-TTS, VoxCPM2, Chatterbox
     Turbo/Nano and Kokoro.
   - There is a written craft canon for writing lines for TTS, accepting renders, and keeping
     a consent ledger.
   - The owner preferred **Qwen3-TTS** by ear for cloned voices. As installed via pip on
     Windows it is **about 3x slower than real time** **[measured]**, so it is a content
     engine, not a live one.
2. **Recommended voice path:**
   - **Design** an original voice from a text description with **Qwen3-TTS VoiceDesign** or
     **VoxCPM2**. No human source is involved.
   - **Bank** a mood-tagged set of reference clips from it.
   - **Speak live** with a cloner that keeps up on the 4090 by cloning from that bank:
     **VoxCPM2 compiled** (0.11 s first audio, RTF 0.47 **[measured]**) or **Chatterbox
     Turbo** (MIT, RTF 0.48 **[measured]**, `[laugh]` tags).
   - **Pre-render** set-piece lines (the booming laugh, the whispered threat) as stingers,
     using IndexTTS2 emotion vectors or Qwen3-TTS `instruct`.
   - **Add the skeletal timbre** with a deterministic FX chain (Spotify pedalboard: pitch,
     formant, rasp, crypt reverb), not by cloning anyone.
3. **Recommended avatar path:**
   - Concept art comes from Leonardo, via the owner's existing `leonardo` skill.
   - Lock the design with reference-guided edits (Nano Banana Pro / Kontext / GPT Image) into
     a T-pose turnaround.
   - Build the model with **Tripo** multiview-to-3D, then retopology, then auto-rig with
     `spec=vrm`, then retarget the villain presets (`cast_a_spell`, `laugh_*`, `angry_*`).
   - Finish in **Blender with the VRM Add-on**: hand-made jaw and eye-glow expressions (a
     skull has no lips).
   - Drive it with **three-vrm in an OBS browser source** over a WebSocket, or VNyan.
   - The 2D/Live2D route is possible but rigging is still manual. Auto-layering (See-through,
     Qwen-Image-Layered) gives a PSD draft at best, and it is trained on anime faces, not
     skulls.
4. **Main risks:**
   - **IP:** a "Skeletor look-alike" is the biggest legal exposure. The SKELETOR trademark is
     registered, and the character is copyrightable under *DC Comics v. Towle*.
   - **Licences:** many strong TTS models are non-commercial (F5, Fish S2-Pro, Spark,
     MaskGCT, Llasa, Breeze 2, Voxtral, Higgs 3).
   - **Vendor speed figures:** they did not reproduce on Windows in the owner's own bake-off.
   - **EU AI Act Art. 50:** disclosure duties have applied since 2026-08-02.
   - **Shared GPU:** TTS, LLM and avatar rendering all compete for one 24 GB card.

---

## Part A — The owner's existing voice references

### A1. What the owner actually runs or has measured

| Engine | Where | Role | Evidence |
|---|---|---|---|
| **Kokoro-82M** (Apache-2.0) via **sherpa-onnx** | `<owner-workspace>/personas\src-tauri\Cargo.toml`; `<owner-workspace>/kp\packages\voice-tts\README.md` (provider `kokoro`, local sherpa sidecar); `<owner-workspace>/athena-portable\README.md`; `<owner-workspace>/.oc-spike\src\src-tauri\Cargo.toml` | Shipped live preset voice (Personas' companion Athena, voice `af_heart`) | Owner verdict 2026-09-30: same quality on CPU, GPU and sherpa-onnx |
| **Piper** | `kp/packages/voice-tts` (provider `piper`) | Local fallback adapter | — |
| **ElevenLabs** (cloud) | `kp`, `firetv*`, `garden-vr`, `gravitone-gcloud`, `gvr-sundial`, `hyper`, `stat-reel`, `mage-arena-tv/tools/audio`, `moderator`, `v`, `vaai` (package.json / README) | Cloud voice and backup; Death Ride campaign dialogue | The owner's direction in Personas: "I would still use ElevenLabs as backup" |
| **Qwen3-TTS 0.6B / 1.7B Base + 1.7B VoiceDesign** | Personas bake-off (`<owner-workspace>/personas\.contest\arena\athena-voice-engines\`) | Chosen by ear for cloned voices, for pre-rendered content | **[measured]** see A2 |
| **VoxCPM2 2B** | same | Top similarity score, overruled by the listener | **[measured]** |
| **Chatterbox Turbo 350M / Nano 110M** | same | Fast GPU cloners. Nano was rejected on CPU time | **[measured]** |
| `@kazm/voice-tts` package | `<owner-workspace>/kp\packages\voice-tts\README.md` | Multi-provider TTS interface: probes, preference, visible fallback, error→HTTP mapping, `X-Tts-Provider` / `X-Tts-Fallback-From` headers | Reusable as Overlord's TTS seam |

Also present in the registry's casting snapshot, but not used: XTTS-v2, Orpheus, NeuTTS Air,
Dia2, Higgs Audio v2, Fish/OpenAudio.

VibeVoice was ingested as a source
(`<owner-workspace>/ai-registry\librarian\sources\2026-09-04-vibevoice.md`). Its
long-form TTS code was **withdrawn** by Microsoft, which leaves only the 0.5B realtime model
with preset voices.

### A2. The bake-off numbers (RTX 4090, Windows 11, 2026-09-29)

Source:
`<owner-workspace>/ai-registry\knowledge\software-engineering\llm-agent\runtime-and-io\voice-io\applications\process--real-time-synthesis-budget.md`.
Raw data is in `personas\.contest\arena\athena-voice-engines\entries\claude-opus_max-engines\variant-1\results.json`.

RTF below 1 means the engine keeps up with playback. *stream* means it returns a first chunk;
*whole* means it returns the finished sentence.

| Engine · mode | GPU RTF | GPU first audio | Peak VRAM |
|---|---|---|---|
| Kokoro-82M · preset | 0.01 | 0.09 s stream | ~1.0 GB |
| Chatterbox Turbo 350M · clone | 0.48 | 1.56 s whole | 3.6 GB |
| Chatterbox Nano 110M · clone | 0.30 | 0.90 s whole | 2.8 GB |
| Qwen3-TTS 1.7B Base · clone | **3.14** | 10.2 s whole | 5.6 GB |
| Qwen3-TTS 0.6B Base · clone | 2.83 | 13.4 s whole | 3.4 GB |
| Qwen3-TTS 1.7B VoiceDesign | 2.90 | 9.5 s whole | 4.8 GB |
| VoxCPM2 2B · eager | 1.62 | 0.26 s stream | 6.1 GB |
| **VoxCPM2 2B · compiled** | **0.47** | **0.11 s stream** | 7.8 GB |

Speaker similarity, measured as ECAPA cosine against the reference (the ceiling, a held-out
sentence of the same speaker, is 0.86):

| Engine | Similarity |
|---|---|
| VoxCPM2 | 0.82 |
| Qwen3-TTS 0.6B | 0.81 |
| Qwen3-TTS 1.7B | 0.75 |
| Chatterbox Turbo | 0.69 |
| Chatterbox Nano | 0.63 |

Every cloner had 0% WER on Whisper large-v3-turbo. Source:
`<owner-workspace>/ai-registry\knowledge\media-generation\audio-generation\generated-speech-acceptance\applications\process--listening-board.md`.

Lessons that transfer directly to Overlord:

- **Vendor speed figures did not reproduce on Windows.**
  - Qwen3-TTS "97 ms first audio" came out as 10.2 s through the pip package, because the
    pip package does not stream.
  - VoxCPM2 only reaches real time **compiled**. The compiler backend has no official Windows
    build; it needed a **community Windows Triton port**, with a 224 s first compile and a
    15.9 GB RAM peak at load.
  - Without the port the engine **silently falls back** to eager mode (RTF 1.62) with nothing
    failing.
  - Pin that port, and keep the slow path declared.
- **Spawning a process per sentence costs 1.5–2.0 s.** Keep the engine **resident** as a warm
  worker.
- **Every installable package lagged its source.** Qwen's tokenizer called the model hub even
  when offline. Chatterbox Nano needed a source install. VoxCPM2's pip release had no seed
  argument.
- **The listener overruled the top score.** VoxCPM2 led on similarity, but it copies the
  reference's delivery, including about a second of silence and a slow pace. Qwen3-TTS was
  preferred. Decisions are per use: a cloner for prepared content, a fast voice for live
  replies.
- **Cloning is positioned for pre-rendered content**, not turn-by-turn speech. In Personas,
  cloning was deferred "until the market develops".

### A3. The craft canon (ai-registry knowledge)

| Lesson | File (under `<owner-workspace>/ai-registry\knowledge\`) |
|---|---|
| **Text carries the emotion, not the settings.** Keep voice settings (stability, exaggeration) at a neutral baseline per voice and write the menace into the line: word choice, sentence length, clipped command against long trailing clause. A setting changes every line the voice speaks. | `game-production\narrative-and-dialogue\line-craft\speech-synthesis-script-writing\techniques\text-carries-the-emotion-not-the-settings.md` (+ application from Death Ride) |
| **Change the text before the settings.** Re-render first, since a fault that does not repeat was the sample. Then edit the line. Then, per voice, touch a setting, and re-hear every approved line. | `...\speech-synthesis-script-writing\techniques\change-the-text-before-the-settings.md` |
| **Hesitation marks can silence a line.** Ellipses, fillers, pause tags and fragments are requests for non-speech, and a render can come back mostly silent. Allow at most one hesitation per line, inside it, and measure the silence ratio. A villain's "dramatic... pause..." is exactly this trap. | `...\techniques\hesitation-marks-can-silence-a-line.md` |
| **Punctuation is direction.** A question mark can turn menace into uncertainty. Sentence boundaries are the strongest control. | `...\techniques\punctuation-as-direction.md` |
| **Length and breath budget.** One short sentence, or two very short ones, per line. Price the pause per boundary from each voice's own renders: median error 0.5 s against 1.3 s with one cast-wide price. | `...\techniques\length-and-breath-budget.md` |
| **Tags depend on the model generation.** `[laugh]` on one engine is spoken as words on another. Re-audition every tagged line whenever the engine changes. | `...\techniques\tags-depend-on-the-model-generation.md` |
| **Round-trip intelligibility.** Pin the ASR by full revision, normalise both sides, score Chinese/Japanese by character, and score the natural recording first to get the recogniser's floor. | `media-generation\audio-generation\generated-speech-acceptance\techniques\round-trip-intelligibility.md` |
| **Listening board.** Group takes by line, with anchors (reference and held-out) as audio, every clip audible, numbers under each player, a listen-for list, and blind, loudness-matched playback. The listener's verdict is an artifact. | `...\generated-speech-acceptance\techniques\listening-board.md` + `applications\process--listening-board.md` |
| **Three axes beside intelligibility.** Similarity, quality and style are separate numbers, never combined into one composite. | `...\generated-speech-acceptance\techniques\three-axes-beside-intelligibility.md` |
| **Chunk-boundary continuity.** Cut at turns, then paragraphs, then sentences, never inside a sentence. Leave about 20% headroom under the engine's character cap. Carry context instead of re-priming. | `media-generation\audio-generation\multi-speaker-speech-production\techniques\chunk-boundary-continuity.md` |
| **World lexicon as pronunciation dictionary.** Invented names (the villain's realm, minions) are pronounced from a lexicon, not by hope. | `...\multi-speaker-speech-production\techniques\world-lexicon-as-pronunciation-dictionary.md` |
| **Voice asset bound to model generation.** A voice is an artifact of one engine generation. | `...\techniques\voice-asset-model-generation-binding.md` |
| **Cast ledger with consent.** One row per voice: role, asset, model generation, origin (designed / licensed / cloned), consent document location, reference material and its terms, status. **A row with an empty consent field refuses the render.** A designed voice reads "not applicable", never blank. | `game-production\asset-production\motion-and-audio\game-dialogue-voice-pipeline\techniques\cast-ledger-with-consent.md` |
| **Bake versus runtime speech, with a stall ladder.** Baked-authored, baked-pool and runtime classes. For runtime lines, play a pre-baked filler in the character's voice, then text-first, then a held "thinking" animation. Fits a villain well: "Silence, worm... let me consult the orb." | `...\game-dialogue-voice-pipeline\techniques\bake-versus-runtime-speech.md` |
| **Re-cast gate.** A new model generation is a re-cast event, not a drop-in. | `...\game-dialogue-voice-pipeline\techniques\re-cast-gate.md` |
| **Authored voice identity.** A described or cloned voice: the **specification is durable, the timbre is volatile**. Store the description, engine version and seed, plus the reference sample, as the system of record. Keep rendered samples as the evidence of what the voice *was*. | `software-engineering\llm-agent\runtime-and-io\voice-io\techniques\authored-voice-identity.md` |
| **Real-time synthesis budget.** RTF plus first-audio *kind* (stream or whole). If an engine cannot stream, feed it sentence by sentence, which only works when RTF < 1. | `...\voice-io\techniques\real-time-synthesis-budget.md` |
| **Casting snapshot, August 2026.** Direction dialects per engine; open weights are "casting-viable" (Kokoro, Chatterbox-Turbo, Orpheus, Dia2, Higgs v2 are commercially clean). | `media-generation\narrative-craft\creator-voice-and-tone\applications\process--spoken-delivery-direction.md` |
| **Audio production recipe.** Trim padding, level once, verify each segment against its words. | `<owner-workspace>/ai-registry\recipes\creative_design\audio\audio-briefing-production\RECIPE.md` |

`catalog.json`, `registry.yaml` and `projects.json` contain **no** TTS entries.

**A note on consent.** In the Personas review
(`personas\.contest\arena\athena-voice-engines\data\owner-review.md`, 2026-09-29) the owner
overrode the consent constraint because "the app is local and not used for commercial use".
**Overlord is a public, potentially monetised livestream, so that override does not carry
over.** The cast-ledger rule applies.

### A4. Owner references for character assets (relevant to Part C)

- **`<owner-workspace>/ai-registry\skills\leonardo\SKILL.md`** (v1.5.3), with tools in
  `skills\leonardo\tools\` (`leonardo-image.mjs`, `leonardo-gpt-image.mjs`,
  `openai-image.mjs`, `gemini-recognize.mjs`, `qwen-image.mjs`, `tight-crop.mjs`).
  - The default generator is OpenAI GPT Image 2.5 Sunburst; Leonardo is the fallback.
  - Measured 2026-09-22: Leonardo's v2 API accepts the slug `gpt-image-2`, but refuses every
    slug tried for GPT Image 2.5.
  - **Two token pools:** `subscriptionTokens` for the web app and `apiSubscriptionTokens` for
    the API. A 402 on the API means the API pool is empty.
  - Lucid Origin has no transparency option, so use `remove-bg` (with `--no-cleanup`).
- **`<owner-workspace>/garden-vr\docs\research\asset-pipeline-and-eval.md`.**
  - Tripo Pro is listed as a paid fallback: $19.90/mo, 3,000 credits (third-party price).
  - Free tiers of Meshy and Tripo are CC BY 4.0, so commercial use needs a paid tier.
  - Pin the model version string per asset class.
  - Hunyuan3D open weights exclude the EU, UK and South Korea.
- **`<owner-workspace>/ai-registry\librarian\sources\2026-09-07-3d-ai-creature-workflow.md`.**
  - Practitioner warning: **Tripo returns models in a different position/orientation from
    the input** (Hunyuan keeps it), which costs a manual realignment per part.
  - When Mixamo auto-rig fails, export the Mixamo skeleton, place the bones by hand, and
    parent with automatic weights.
- **`ai-registry\knowledge\game-production\asset-production\geometry\mesh-finishing-for-engine-readiness\techniques\rig-preset-and-bone-remap-binding.md`**
  and **`...\production-governance\generative-provider-auditing\applications\node--pin-a-model-per-asset-class.md`.**

---

## Part B — Local TTS state of the art (October 2026)

### B1. Comparison table

**Commercial column.** "Yes" means both code and weights allow commercial use. Thresholds are
noted where they apply. VRAM marked "~" with no source is an estimate (UNVERIFIED).

| Model | Licence (code / weights) | Commercial | VRAM | Streaming / first audio | Cloning | Emotion control | Languages |
|---|---|---|---|---|---|---|---|
| **Chatterbox** 500M | MIT / MIT | Yes | ~4–7 GB (UNVERIFIED) | Not native; streaming fork 0.47 s first chunk, RTF 0.5 on a 4090 | ~10 s zero-shot | `exaggeration` knob (0.7+ for drama), `cfg_weight` | en |
| **Chatterbox Turbo** 350M | MIT / MIT | Yes | 3.6 GB **[measured]** | RTF 0.48, 1.56 s whole **[measured]** | zero-shot | native `[laugh] [chuckle] [cough]` tags | en |
| Chatterbox Multilingual V3 | MIT | Yes | ~5 GB (UNVERIFIED) | not native | zero-shot | exaggeration | 23+ |
| **IndexTTS2 / 2.5** (bilibili) | bilibili Model Use License | Yes below 100M MAU / RMB 1B revenue; may not be used to improve other models | fp16 | no native streaming; RTF 0.33 (v2) / 0.21 (v2.5) on a 4090 **[vendor]** | ~5 s zero-shot | **best disentanglement:** 8-dim emotion vector, separate emotion reference with `emo_alpha`, or emotion from text | v2 zh/en; v2.5 + ja/es/ar |
| Higgs Audio v2 3B | Apache / Boson community (Llama-derived) | Yes below 100k annual active users | 24 GB recommended | vLLM OpenAI server | zero-shot | automatic prosody | multi |
| Higgs TTS 3 4B (Jun 2026) | research/NC + Creator Use Grant | Unclear for livestreams | — | SSE, sub-second | zero-shot | 21 emotion tokens | 102 |
| VibeVoice | MIT, but "research only" | No (TTS code pulled 2025-09-05) | — | Realtime-0.5B ~300 ms | **no** (presets only) | — | en |
| Fish S2-Pro / Drama 3 | Fish Research / Community License | Community: below $500k revenue, no hosted service, **no training on outputs**; S2-Pro needs an agreement | consumer GPU | ~100 ms (H200) | yes | `[whisper] [angry] [laughing]` | ~100 |
| OpenAudio S1-mini | Apache / CC-BY-NC-SA | **No** | — | — | yes | markers | multi |
| **Fun-CosyVoice3-0.5B** | Apache / Apache | Yes | ~4–6 GB (UNVERIFIED) | bi-streaming ≥150 ms **[vendor]**, vLLM / TRT-LLM | zero-shot, cross-lingual | natural-language instructions | 9 + 18 dialects |
| F5-TTS v1 | MIT / CC-BY-NC | **No** | — | RTF 0.15 (L20) | zero-shot | reference only | zh/en |
| **Kokoro-82M** | Apache | Yes | <1 GB | 0.09 s stream **[measured]** | **none** | presets + weighted blends | 8 |
| Orpheus 3B | Apache (Llama-derived) | Mostly (UNVERIFIED) | ~8–16 GB | ~200 ms | weak | `<laugh> <sigh> <groan> <gasp>` | en |
| Dia2 1B/2B | Apache | Yes | — | starts before full text; server "upcoming" | audio prefix | nonverbal (Dia1) | en |
| Sesame CSM-1B | Apache | Yes (consent required for real people) | — | — | context | context | en |
| **Qwen3-TTS** 0.6B/1.7B (Base / CustomVoice / **VoiceDesign**) | Apache-2.0 | Yes | 3.4–5.6 GB **[measured]** | **pip on Windows: whole, RTF ~3 [measured]**; vLLM-Omni WebSocket / faster-qwen3-tts ~154–171 ms TTFA on a 4090 (community, not verified on Windows) | 3 s | natural-language `instruct`; **VoiceDesign** from a description | 10 |
| Spark-TTS 0.5B | Apache / CC BY-NC-SA | **No** | — | — | zero-shot | pitch/speed parameters | zh/en |
| Zonos v0.1 | Apache | Yes | 6 GB+ | ~2x real time on a 4090; Windows via fork | 10–30 s | emotion conditioning | 5 |
| MaskGCT, Llasa-3B | CC-BY-NC | **No** | — | — | yes | — | zh/en |
| Kyutai TTS 1.6B | MIT / CC-BY-4.0 | Yes | — | Rust websocket (Unmute) | **pre-computed voices only** | — | en/fr |
| NeuTTS Air | Apache | Yes | CPU/GGUF | GGUF streaming | 3 s | — | en |
| **Step-Audio-EditX** 3B | Apache code (weights licence not stated) | Probably (UNVERIFIED) | ~12–16 GB | not documented | zero-shot | **iterative edit of existing audio:** arrogant, roar, shout, whisper, coldness, authority, exaggerated; `[laugh] [snort]` | zh/en/ja/ko |
| Kani-TTS-2 | Apache | Yes | 4–8 GB | RTF 0.2–0.5 | 128-dim embedding (averaging) | — | en (+de/fr) |
| **Maya1** 3.3B | Apache | Yes | 16 GB+ | <100 ms vLLM **[vendor]** | **no zero-shot** | **voice from description** + `<angry> <mischievous> <sarcastic> <laugh_harder> <scream> <whisper>` | en |
| **VoxCPM2** 2B | Apache / Apache | Yes | 6.1–7.8 GB **[measured]** | **compiled 0.11 s stream, RTF 0.47 [measured]** (needs community Windows Triton); Nano-vLLM RTF 0.13 **[vendor]** | clone with style guidance; "ultimate" clone with transcript | **voice design** + style text | 30 |
| Breeze TTS 2 3B | Apache / research-NC | **No** | 12–24 GB | <40 ms (H100) | yes | design + direction | en/zh |
| Voxtral TTS 4B (Mistral) | CC BY-NC | **No** (commercial use via API) | — | — | UNVERIFIED | — | 9 |
| Soprano-80M | UNVERIFIED | ? | tiny | <15 ms **[vendor]** | no | — | en |

Unverified 2026 names (one roundup, no primary source): FireRedTTS3, MOSS TTS 1.5, OmniVoice,
Audio8, Sopro V2 Turbo (https://openvoxai.com/blog/best-tts-models-local-cloud-2026).

Sources:
- Chatterbox: https://github.com/resemble-ai/chatterbox · https://huggingface.co/ResembleAI/chatterbox-turbo · https://github.com/davidbrowne17/chatterbox-streaming
- IndexTTS: https://github.com/index-tts/index-tts (+ LICENSE)
- Higgs: https://github.com/boson-ai/higgs-audio (README_V2) · https://huggingface.co/bosonai/higgs-tts-3-4b
- VibeVoice: https://github.com/microsoft/VibeVoice
- Fish Speech: https://github.com/fishaudio/fish-speech (LICENSE)
- CosyVoice: https://github.com/FunAudioLLM/CosyVoice · https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512
- https://github.com/SWivid/F5-TTS · https://huggingface.co/hexgrad/Kokoro-82M · https://github.com/remsky/Kokoro-FastAPI
- https://github.com/canopyai/Orpheus-TTS · https://github.com/nari-labs/dia2 · https://github.com/SesameAILabs/csm
- Qwen3-TTS: https://github.com/QwenLM/Qwen3-TTS · https://arxiv.org/html/2601.15621 · https://pypi.org/project/faster-qwen3-tts/0.3.0/
- https://huggingface.co/SparkAudio/Spark-TTS-0.5B · https://github.com/Zyphra/Zonos · https://huggingface.co/amphion/MaskGCT · https://huggingface.co/HKUSTAudio/Llasa-3B
- Kyutai: https://huggingface.co/kyutai/tts-1.6b-en_fr
- https://github.com/neuphonic/neutts · https://github.com/stepfun-ai/Step-Audio-EditX · https://github.com/nineninesix-ai/kani-tts-2
- https://huggingface.co/maya-research/maya1 · https://github.com/OpenBMB/VoxCPM
- Breeze TTS 2: https://www.mindstudio.ai/blog/breeze-tts-2-open-weight-model
- Voxtral TTS: https://slator.com/mistral-text-to-speech-model/

**Licence traps when chaining tools.**
- Fish forbids training speech models on its outputs, so do not train RVC on Fish audio.
- bilibili forbids using IndexTTS outputs or the model to improve other models.
- Chatterbox and NeuTTS embed **Perth watermarks** by default, which is useful for EU AI Act
  Art. 50(2) machine-readable marking.

**Windows.** vLLM, SGLang and Zonos are Linux-first. The serving stacks behind the fast
vendor numbers (vLLM-Omni for Qwen3-TTS, Nano-vLLM for VoxCPM2, vLLM for CosyVoice and Higgs)
probably need **WSL2** (UNVERIFIED for each one).

### B2. Streaming servers

| Server | Model | API | First audio |
|---|---|---|---|
| https://github.com/travisvn/chatterbox-tts-api | Chatterbox (+ML; Turbo "coming soon") | OpenAI `/v1/audio/speech`, `/stream` chunked or SSE, voice library, Docker | "1–2 s" (claim) |
| https://github.com/davidbrowne17/chatterbox-streaming | Chatterbox | Python generator | 0.47 s on a 4090 |
| https://docs.vllm.ai/projects/vllm-omni/en/stable/serving/speech_api/ | Qwen3-TTS (all variants) | OpenAI + **PCM WebSocket streaming**, voice upload | 97 ms vendor; ~154–171 ms community |
| CosyVoice built-in | CosyVoice 2/3 | gRPC / FastAPI, vLLM, TRT-LLM | ≥150 ms vendor |
| https://github.com/Ksuriuri/index-tts-vllm | IndexTTS 1/1.5/2 | OpenAI `/audio/speech` ("may have bugs") | no streaming; s2mel is the bottleneck |
| https://github.com/Lex-au/Orpheus-FastAPI | Orpheus GGUF | OpenAI, WAV | not published |
| Kokoro-FastAPI | Kokoro | OpenAI-compatible, blends | very fast |
| Kyutai Unmute | Kyutai TTS | Rust websocket | audio trails text by 1.28 s |

Owner's own seam: `<owner-workspace>/kp\packages\voice-tts`. It is a provider interface
with probes and visible fallback; adding a local HTTP-streaming adapter there is the natural
integration point.

### B3. Recommended engine plan for Overlord

All numbers come from the bake-off unless marked otherwise.

| Job | Engine | Why |
|---|---|---|
| **Voice creation** (one-off) | **Qwen3-TTS 1.7B VoiceDesign** (the owner already ran it, 4.8 GB) and/or **VoxCPM2 voice design** | Apache; builds a voice from prose with no human source, so the consent row reads "designed — n/a" |
| **Live speech** (chat replies) | **VoxCPM2 compiled** (0.11 s streamed first audio, RTF 0.47) cloning from the designed reference bank; fallback **Chatterbox Turbo** (RTF 0.48, MIT, `[laugh]` tags; feed sentence by sentence) | Both keep up on the 4090 on Windows. Re-check VoxCPM2's habit of copying the reference's pauses: give it a tight, energetic reference clip |
| **Try next** | **faster-qwen3-tts** or **vLLM-Omni (WSL2)** for Qwen3-TTS streaming | The owner preferred Qwen3's sound; ~170 ms community figure, **unverified on this machine**. Bench it before adopting |
| **Set pieces** (baked stingers, intros, the laugh library) | **IndexTTS2** emotion vectors (angry / disgusted / surprised blends), **Qwen3-TTS `instruct`**, or a **Step-Audio-EditX** "arrogant / roar / whisper" pass | Quality over latency; listened to before use (listening board) |
| **Stall filler** | Pre-baked pool of in-character fillers ("Hmm... the orb stirs.") | Follows bake-versus-runtime-speech |
| **Emergency fallback** | Kokoro (presets/blend) + FX chain, or ElevenLabs (already wired in kp) | Visible fallback, never silent |

**VRAM budget.** The local LLM, TTS (~4–8 GB), and any avatar or talking-head model share
24 GB. Budget it explicitly. A 4-bit ~14B LLM (~10 GB) plus VoxCPM2 (~8 GB) fits; a 32B LLM
does not fit with it (estimate, UNVERIFIED).

### B4. Creating a novel voice legally

1. **Voice design from text (preferred).**
   - Tools: Qwen3-TTS VoiceDesign, VoxCPM2 design, Maya1 (description plus villain tags, but
     no cloning and 16 GB+). Parler-TTS works but is inconsistent between generations.
   - ElevenLabs Voice Design is the commercial reference
     (https://elevenlabs.io/docs/help-center/product/voices/voice-design/what-is-voice-design).
   - Example prompt: *"Older male, deep, dry and gravelly, theatrical stage diction,
     precise consonants, sardonic, savours words before a cruel punchline, slight hiss on
     sibilants, no accent of a real person."*
   - Render 20–60 s in several moods (neutral gloat, sneer, rage, whisper, cackle).
   - Store **description + engine version + seed + clips** as the system of record
     (authored-voice-identity). The timbre drifts between regenerations, so the banked clips
     are the voice.
2. **Embedding blending.**
   - Kokoro-FastAPI supports weighted blends saved as `.pt` voicepacks.
   - Kani-TTS-2 documents averaging its 128-dim embeddings.
   - Chatterbox `speaker_emb` interpolation is plausible but has no official example
     (UNVERIFIED).
   - Cartesia documents linear mixing and notes that perceived mixes are nonlinear
     (https://docs.cartesia.ai/concepts/embeddings-and-voice-mixing).
   - RVC can merge checkpoints.
   - Blend only voices whose licence allows derivatives. Never blend a real person's
     embedding in to "get close".
3. **A consenting voice actor.**
   - Use a written release covering AI training, synthetic real-time use, scope, duration,
     pay and revocation, and keep a recorded spoken consent statement (Resemble / ElevenLabs
     practice: https://knowledge.resemble.ai/why-do-i-need-to-give-consent).
   - California AB 2602 requires a reasonably specific description of uses
     (https://www.skadden.com/insights/publications/2024/09/california-enacts-host-of-ai-related-bills).
   - SAG-AFTRA's 2025 Interactive Media Agreement requires informed consent per use and
     **≥7.5x minimum scale for real-time generation** (https://www.thewrap.com/sag-aftra-video-game-deal-details/).
     That is a fair-pay benchmark even for non-union talent.
   - The actor performs *original* villain material, not a Skeletor impression.
4. **Voice conversion on top of TTS** (optional).
   - RVC v2 (MIT): ~90–170 ms real-time, trained on ≤10 min
     (https://github.com/RVC-Project/Retrieval-based-Voice-Conversion-WebUI).
   - Applio (RVC fork, formant shift, maintenance mode).
   - Seed-VC: GPL-3.0, archived Nov 2025, ~400 ms.
   - OpenVoice v2 (MIT).
   - Train only on audio you own (the designed voice, or the actor with consent). For a
     designed voice, VC mostly adds latency; the FX chain is usually enough.
5. **Skeletal timbre as deterministic post-FX.** Spotify pedalboard has an `AudioStream` for
   live use and hosts VST3 on Windows (https://github.com/spotify/pedalboard). The chain
   below is a starting point, not tested:
   1. PitchShift −2 to −4 semitones
   2. Formant down (VST3 or RVC/Applio toggle)
   3. Light Distortion/Clipping for rasp
   4. Low-mix Bitcrush
   5. High-pass around 80 Hz and a presence boost
   6. Short doubling Delay or Chorus for hollowness
   7. Convolution reverb with a crypt/cavern IR
   8. Compressor, then Limiter

   It adds a few ms per chunk. Pedalboard is GPL-3.0, which only matters if a closed
   binary is distributed.

### B5. Legal risk of cloning the real Skeletor voice (do not)

- **Who it would be copying.** The voices are Alan Oppenheimer (1980s Filmation), Mark Hamill
  (*Revelation*) and Jared Leto (2026 film). The film's director deliberately avoided
  imitating Oppenheimer (https://www.toy-people.com/en/?p=109279).
- **Right of publicity in voice.**
  - Tennessee **ELVIS Act** (2024): covers voice, plus liability for tools built for
    unauthorised replicas.
  - California **AB 1836**: deceased performers' digital replicas need estate consent.
  - California **AB 2602**: limits replica clauses in contracts.
  - California Civ. Code §3344: general publicity right (citation UNVERIFIED).
- **NO FAKES Act** (S.1367): passed Senate Judiciary unanimously in June 2026 but is **not
  law** (https://www.blackburn.senate.gov/index.php/2026/6/blackburn-coons-bipartisan-bill-to-protect-individuals-and-creators-from-deepfakes-passes-senate-judiciary-committee).
- **EU AI Act Art. 50**, applicable from 2026-08-02 (https://artificialintelligenceact.eu/article/50/):
  - **50(4):** deployers must disclose deepfakes. A clone of a real actor qualifies; an
    original voice largely does not. For evidently fictional works, a light disclosure is
    enough.
  - **50(2):** machine-readable marking of synthetic audio (provider duty).
  - **50(1):** disclose interaction with an AI unless obvious. This bears on an autonomous
    streamer chatting with viewers (paraphrased from memory; UNVERIFIED wording).
- **Platforms.**
  - **YouTube:** requires disclosure of realistic synthetic content (a real person's voice)
    and has an impersonation policy (https://blog.youtube/news-and-events/disclosing-ai-generated-content).
  - **Twitch:** no AI-disclosure rule as of Sept 2026 (https://gamesbeat.com/twitch-ceo-dan-clancy-says-platform-doesnt-need-ai-disclosure-rules-for-now/),
    but its impersonation rules apply.
  - **Kick:** reportedly requires AI disclosure since March 2026 (UNVERIFIED).
- **Model licences.** Fish, Higgs 3, CSM and VibeVoice ban cloning real people without
  consent.
- **Practical step.** Put a permanent "AI character · synthetic voice" note on the overlay and
  channel bio.

---

## Part C — Character model generation and avatar pipeline

### C1. Leonardo.ai API (owned by Canva since July 2024)

**Models.** Source: https://docs.leonardo.ai/llms.txt and https://docs.leonardo.ai/docs/commonly-used-api-values.
- **Own image models:** Lucid Origin, Lucid Realism, Phoenix 1.0/0.9, SDXL-era (Kino XL,
  Anime XL, 3D Animation Style).
- **Third-party image models:** FLUX Dev/Schnell, FLUX.1 Kontext [pro]/Max, FLUX.2 Pro;
  Seedream 4.0/4.5/5.0; Nano Banana / Pro (`gemini-image-2`) / 2 / 2 Lite; GPT Image
  1.5 / 2 / 2.5 Flare / 2.5 Sunburst.
- **Video:** Veo 3.1, Kling 2.1–3.0/O-series, Motion 2.0, FLUX 3 Video.
- **3D:** Rodin V2 and Meshy 7, both through Leonardo's API.
  - Rodin V2: 1–5 references → GLB, PBR, quad or tri, `ta_pose` flag, **no rigging**
    (https://docs.leonardo.ai/docs/rodin-v2.md).
  - So Leonardo alone could make the mesh, but Tripo is still needed for the rig.

The owner's skill notes that the v2 endpoint `model` field is an enum of Leonardo slugs, and
that GPT Image 2.5 slugs were refused on 2026-09-22.

**Consistency features.**
- **Phoenix v2 `guidances`:** `character` (1 ref, LOW/MID/HIGH), `content`, `style` (up to
  4), `image_to_image` (https://docs.leonardo.ai/docs/phoenix).
- **SDXL v1:** Character Reference is controlnet preprocessor 133 and Style Reference is 67.
- **Nano Banana Pro:** up to 6 `image_reference` slots
  (https://docs.leonardo.ai/docs/nano-banana-pro.md).
- **Kontext:** single and multi-image edit recipes.
- **Elements (LoRA) training via API:** documented on SDXL. Flux-based Elements on the API
  are UNVERIFIED (https://docs.leonardo.ai/docs/custom-model-training).
- **Best practice:** lock one canonical design, then derive the front T-pose, a 3/4 view,
  side, back, an expression/jaw sheet and prop sheets by **reference-guided edit** (Nano
  Banana Pro / Kontext / GPT Image) rather than re-prompting.

**Utilities.**
- `remove-bg`: RGBA PNG/WebP or mask, ≤10 MP, about $0.10 per image in the doc example
  (https://docs.leonardo.ai/docs/remove-bg.md).
- Pro Upscaler (Precise/Creative, May 2026) and Universal Upscaler.

**Pricing.**
- The API is now **pay-as-you-go**: a dollar balance that never expires, top-ups, and a
  `cost` object in each response (https://docs.leonardo.ai/docs/pricing-and-plans-faq).
- The legacy API Basic/Standard/Pro plans are being phased out; third-party figures conflict
  (UNVERIFIED).
- Per-model cost: UNVERIFIED; use the pricing calculator.
- The owner's skill notes that the API and web token pools are separate.

**Rights and moderation.**
- API outputs are reportedly assigned to the customer and not used for training. This comes
  from a search snippet of https://www.leonardo.ai/api, which returned 403 (UNVERIFIED).
- Paid users keep IP on their outputs; free-tier outputs belong to Leonardo
  (https://intercom.help/leonardo-ai/en/articles/8044018-commercial-usage).
- The terms prohibit infringing content and give **no warranty against IP claims**.
- An NSFW filter returns 400 errors and an `nsfw` flag.
- Whether named-character prompts are filtered is UNVERIFIED. **Filter "Skeletor/He-Man/
  Mattel/Grayskull" in Overlord's own prompt layer.**

### C2. Tripo API (v3)

Base URL `https://openapi.tripo3d.com/v3`, Bearer auth, async tasks with polling or webhooks
(https://developers.tripo3d.com/en/docs/introduction).

**Models** (https://developers.tripo3d.com/en/models, list "updated 2026-05"):
- **H3.1:** flagship, ~40 s, PBR, March 2026.
- **P1:** low-poly, ~10 s.
- Tripo 3.0: Sept 2025.
- Rigging and Animation models.

**Task types:**
- 3D generation: text / image / **multiview**-to-3D, image-to-Gaussian.
- Image generation: Tripo resells Seedream, Nano Banana and GPT-image; also image-to-multiview.
- Processing: texture (standard, HD, 8K), segmentation, part completion, **retopology
  (tri or quad)**, smart UV, convert.
- Animation: rig check, auto-rig, retarget.
- "Stylize": UNVERIFIED in v3.

**Rigging** (https://developers.tripo3d.com/en/docs/animations-rig):
- Versions:
  - `v1.0-20240301`: biped only.
  - `v2.5-20260210`: creatures.
  - `v3.0-20260909`: better humanoid rigging; request it explicitly.
- Types: biped, quadruped, hexapod, octopod, avian, serpentine, aquatic.
- Biped `spec`: tripo / **mixamo** / actorcore / unity / unreal / **vrm** (some specs need
  v3.0).
- Input is a Tripo task or an uploaded GLB/FBX/OBJ/STL up to 150 MB.

**Retargeting** (https://developers.tripo3d.com/en/docs/animations-retarget): v1.0 has 90+
biped presets, including **`cast_a_spell`, `angry_01–03`, `laugh_01/02`, `idle`, `swagger`,
`fold_arms`, `victory_celebration`, `bow`, `greet_*`**. That is a ready villain gesture
library.

**Export.** GLB and FBX natively. A convert task adds GLTF/USDZ/OBJ/STL/3MF (per third-party
wrappers and ComfyUI docs). **There is no direct `.vrm` export**; `spec=vrm` gives
VRM-compatible bone naming only.

**Pricing** (https://developers.tripo3d.com/en/pricing), in credits:

| Task | Credits |
|---|---|
| Text-to-3D (no texture / textured) | 10 / 20 |
| Image- or multiview-to-3D (no texture / textured) | 20 / 30 |
| Add-on: HD texture | +10 |
| Add-on: HD geometry | +20 |
| Add-on: quad | +5 |
| Add-on: smart low-poly | +10 |
| Add-on: parts | +20 |
| Auto-rig | 25 |
| Retarget (per animation) | 10 |
| Rig check | free |
| Retopology (tri / quad) | 35 / 40 |
| Segmentation | 40 |

- Price per credit is about **$0.01**, from Tripo's 2024 announcement and the AWS Marketplace
  overage rate. The current USD rate is UNVERIFIED.
- Failed tasks are refunded. Web and API credits are separate.
- **One rigged, textured, quad character with 5 gestures is roughly 30 + 40 + 25 + 50 ≈ 145
  credits ≈ $1.50** (estimate).

**Rate limits** (https://developers.tripo3d.com/en/docs/rate-limits): concurrency H-series 10,
P-series 5, image 1, animation 10, processing 5. Over-limit requests get 429 with
`Retry-After`.

**Rights.** Paid users get commercial rights and their data is not used for training;
free-tier output is retained by Tripo. This is from a snippet; the help page returned 403.
API-specific terms are UNVERIFIED.

**Tooling.**
- Official JS/TS, Go, Rust and Java SDKs for v3. The Python `tripo3d==0.4.2` is mainly v2
  (https://developers.tripo3d.com/en/docs/sdk).
- **`tripo-mcp`** (MIT, alpha, Blender addon bridge): https://github.com/VAST-AI-Research/tripo-mcp.
- ComfyUI nodes.

**Known gotcha.** A practitioner reports that Tripo returns meshes **re-posed/re-oriented**
relative to the input
(`ai-registry\librarian\sources\2026-09-07-3d-ai-creature-workflow.md`). Normalise transforms
in Blender before rigging or parts work.

### C3. Route 1: 3D rigged VRM avatar (recommended)

1. **Concept.** Leonardo, via the owner's `leonardo` skill: canonical design, then
   reference-edited T-pose turnaround (front, side, back) plus a prop sheet. No franchise
   names in prompts (C5).
2. **Mesh.**
   - Tripo **multiview-to-3D (H3.1)**, then **retopology (quad)**, then the HD texture.
   - Alternative: Leonardo's Rodin V2 with `ta_pose`, followed by a Tripo rig of the uploaded
     GLB.
3. **Rig.**
   - Tripo auto-rig: `rig_type=biped`, `spec=vrm` (v3.0), or `spec=mixamo`.
   - Then retarget the villain presets.
   - If auto-rig fails on the skeletal anatomy: place a Mixamo skeleton by hand and parent
     with automatic weights (practitioner fallback, A4).
4. **VRM.**
   - Blender + **VRM Add-on for Blender** (v4.5.0, July 2026; Blender 4.2 LTS and up;
     https://extensions.blender.org/add-ons/vrm/versions/), or UniVRM in Unity.
   - Map humanoid bones, set spring bones for cape, robe and tattered cloth, and author
     expressions.
5. **Face expressions for a skull.**
   - Tripo meshes most likely have **no blendshapes** (UNVERIFIED).
   - Skip ARKit 52 and hand-make about 3 shape keys:
     - jaw-open large → VRM `aa` / `oh`
     - jaw-open small → `ih` / `ou` / `ee`
     - eye-socket glow intensity via VRM **material binds** → `blink`, `angry`, `happy`
   - VRM expressions bind morph targets and materials, not bones, so make the jaw a shape key
     or drive the jaw bone with custom code. This is from spec memory, not re-verified.
   - Faceit (https://faceit-doc.readthedocs.io/en/latest/) or ARKit Creator are options if a
     fuller face is ever needed.
6. **Runtime.**
   - **three-vrm in an OBS browser source:** a WebSocket from the Overlord orchestrator sends
     TTS amplitude or visemes (`expressionManager.setValue('aa', w)`), gesture triggers that
     play the retargeted clips, and eye-glow emotion states. It is fully headless and the
     most programmable option
     (https://pixiv.github.io/three-vrm/docs/classes/three-vrm-core.VRMExpressionManager.html).
   - **VNyan:** native WebSocket receiver `ws://localhost:PORT/vnyan` driving node graphs;
     there is a community VNyan MCP
     (https://github-wiki-see.page/m/Suvidriel/VNyanDoc/wiki/How-To).
   - **Warudo:** blueprints, C# plugins, uLipSync from audio (route TTS through a virtual
     audio cable). An official WebSocket API is UNVERIFIED
     (https://docs.warudo.app/docs/blueprints/overview).
   - Unreal MetaHuman is a heavier alternative, not researched.

### C4. Route 2: 2D Live2D (possible, more manual)

- **Auto-layering (2026).**
  - **See-through** (arXiv 2602.03749, SIGGRAPH 2026; https://github.com/shitagaki-lab/see-through)
    splits an anime image into semantic, inpainted layers with draw order. In the authors'
    words it is *not* image-to-Live2D; testers call it a "PSD draft generator".
  - **Qwen-Image-Layered** (Apache-2.0, Dec 2025; https://github.com/QwenLM/Qwen-Image-Layered)
    gives 3–10 RGBA layers with PSD export.
  - **Bunraku** (arXiv 2607.27348, July 2026) gives layers, a deformation mesh and keyposes
    (not independently checked).
  - **All of these are trained on anime characters.** A skeletal villain is off their
    training distribution, so expect manual cleanup (inference).
- **Live2D Cubism's own semi-automation.** 5.0 added auto face deformers, 5.1 added
  whole-body deformer and sway generation, and 5.3 (Jan 2026) added no new AI features
  (https://docs.live2d.com/en/cubism-editor-manual/new-function5-1/). **No AI tool auto-rigs
  Live2D end to end as of Oct 2026** (best evidence; UNVERIFIED as a negative claim).
- **Driving it.** The **VTube Studio WebSocket API** (port 8001) injects `MouthOpen` and
  other parameters from the TTS loop. Injected parameters reset unless streamed
  continuously.
- **Cheapest viable look.** A **PNGtuber** (Veadotube mini, PNGTuber Remix) with two or three
  jaw frames and an eye-glow state, driven by TTS loudness. It suits a skull and can ship on
  day one while the 3D asset is built.
- **Neural talking heads** (LivePortrait 12.8 ms/frame on a 4090; MuseTalk 1.5; Ditto;
  SoulX-FlashHead): they need human-like faces and lips. **A lipless skull is likely to
  break them** (UNVERIFIED; test before committing). They would also compete with TTS and
  the LLM for VRAM.

### C5. IP: Skeletor / He-Man is Mattel IP

- **Character copyright.**
  - *DC Comics v. Towle*, 802 F.3d 1012 (9th Cir. 2015): a character is protected if it has
    physical and conceptual qualities, is recognisable across appearances, and is
    "especially distinctive", not a stock character. Skeletor meets this.
  - *Warner Bros. v. X One X*, 644 F.3d 584 (8th Cir. 2011): turning a protected character
    into new 3D or merchandise forms infringed.
  - Sources: https://en.wikipedia.org/wiki/DC_Comics_v._Mark_Towle ·
    https://jolt.law.harvard.edu/digest/warner-bros-entm-e2-80-99t-v-x-one-x-productions
- **Trademark.** SKELETOR is US Reg. 1,212,574 (1982, renewed, toys; https://trademark.justia.com/733/45/skeletor-73345633.html).
  Mattel has enforced against the skeletor.com domain. "Masters of the Universe"
  registrations are UNVERIFIED.
- **What is free (scènes à faire, idea/expression).** A skull-faced sorcerer, a hooded or
  crowned dark lord, a ham-fisted monologue, world-domination schemes, a staff, a lair, and
  incompetent minions are stock tropes (*Nichols v. Universal*, *Walker v. Time Life*).
- **What to avoid (distinctive expression).**
  - The **yellow/gold skull on a blue muscular body**.
  - A purple hood with that silhouette.
  - A **ram-skull staff** (Havoc Staff).
  - Snake Mountain or Castle Grayskull.
  - The name, "Myah!", "By the power of Grayskull".
  - A blond heroic nemesis.
  - An impression of Oppenheimer's or Hamill's delivery.
- **"Inspired by" safely means:** change several axes at once.
  - Palette: e.g. bone-white and verdigris, or obsidian and ember.
  - Anatomy: a full skeleton or armoured ribcage rather than a skull head on a bodybuilder.
  - Headgear: a crown, horns or a broken halo instead of a hood.
  - Signature prop: e.g. an orb, a cracked sceptre or a phylactery.
  - An original name, realm, minions and catchphrases.
- **Generators.**
  - Never prompt with franchise names. The Disney/Universal/WBD v. Midjourney litigation
    (filed June 2025) turns on generators producing named-character derivatives; its current
    status is UNVERIFIED.
  - Block those terms in both the image prompt layer and the LLM persona prompt, so the live
    character never claims to be Skeletor.
  - Run a reverse-image "does this read as Skeletor?" check, a VLM judge plus a human, on the
    final design.
- **Platforms.** Twitch DMCA: three strikes, and blatant infringers can be terminated sooner.
  YouTube copyright strikes (not re-fetched). Leonardo and Tripo both prohibit infringing use
  and give no IP warranty.

Not legal advice. Have counsel review the final design and name before monetising.

---

## Risks and open questions

1. **IP look-alike.** This is the dominant risk. Mitigate with a multi-axis original design, a
   prompt blocklist, a persona-prompt guard and a counsel review.
2. **Licence drift.** Several top TTS models are NC or research-only, and some 2026 releases
   changed licence (Higgs 3, Fish S2-Pro). Keep a licence column in the cast ledger and
   re-check on every engine upgrade (re-cast gate).
3. **Windows serving gap.** The fast numbers need vLLM/SGLang (WSL2) or the community Triton
   port, and VoxCPM2's eager fallback is silent. Assert the compiled path at startup.
4. **VRAM contention.** LLM + TTS + avatar on one 4090. Bench the full stack together, not
   each engine alone.
5. **Expressive range versus stability.** Exaggeration knobs and emotion tags destabilise
   calm lines. Follow text-before-settings, and keep the big emotions in baked stingers.
6. **Silent and garbled renders on unscripted LLM text.** Run a cheap silence-ratio and
   duration screen plus sampled ASR round-trip on live output. Stall-ladder filler covers
   late renders.
7. **EU AI Act Art. 50 disclosure.** Keep a permanent "AI character / synthetic voice" label;
   Chatterbox/NeuTTS Perth watermarks help with marking.
8. **Tripo specifics.** No VRM export, re-oriented meshes, no face blendshapes (UNVERIFIED),
   and a skeletal body may confuse the biped auto-rig. Budget Blender time.
9. **Unverified prices.** Leonardo per-model API cost and Tripo's current $/credit. Confirm
   both in the dashboards before budgeting.
