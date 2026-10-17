# 02 — Tech stack, component by component

Research date: 2026-10-17. Scope: the runtime stack for "Overlord", a fully autonomous, programmatically driven
villain VTuber (no human face tracking) that streams, talks to chat, drives a browser on screen and auto-clips
itself. Hardware assumption: one dedicated Windows 11 Home box (RTX 4090 24 GB, Ryzen 7 7800X3D, 64 GB RAM),
optionally a second notebook. Claude Code CLI is the orchestrator; Leonardo.ai (2D) and Tripo (3D) generate the
character.

Legend: **[V]** verified against a primary source (official docs, repo, model card) during this research.
**[S]** secondary source only (blogs, vendor comparisons, forums). **[UNVERIFIED]** my own knowledge or inference,
not confirmed in this session — check before you rely on it.

---

## 0. TL;DR — recommended stack

| Layer | Recommendation (MVP) | Upgrade path / alternative |
|---|---|---|
| Avatar runtime | **Own browser renderer, used as an OBS Browser Source**: `three-vrm` (VRM, MIT) for 3D, or a layered-sprite "PNGtuber+" in PixiJS for a day-1 2D look | Live2D in VTube Studio (best 2D look, excellent API) if you commission a rig; Warudo if you want a no-code 3D studio |
| Character asset pipeline | Tripo → rig check → rig (biped, `mixamo` spec) → FBX/GLB → Blender + VRM Add-on → VRM 1.0 with hand-built expressions | Leonardo → See-through layer decomposition → manual Live2D rig in Cubism (or commissioned) |
| Mouth / face | Drive from **our own TTS audio**: jaw-open from RMS envelope + optional vowel visemes (uLipSync/MFCC-style) computed **before** playback | Rhubarb per utterance (offline), Audio2Face-3D v3.0 (ARKit-52, overkill for a skull) |
| Voice | Local TTS with cloning — Chatterbox-Turbo (MIT) or Qwen3-TTS (Apache-2.0); IndexTTS-2.x for emotion control (check licence) | Optional RVC voice-conversion pass for the "villain rasp" |
| Broadcasting | OBS Studio 32.1.x + built-in obs-websocket 5.x; Aitum Vertical for a 9:16 canvas; multistream via Aitum Multistream (or Restream) | Twitch Enhanced Broadcasting (multi-encode), Twitch dual-format (alpha) |
| Chat | One normaliser service: Twitch EventSub (WebSocket → Conduit later) + Helix; YouTube `liveChatMessages.streamList` (gRPC); Kick webhooks via a tunnel | — |
| Computer use on stream | Playwright MCP (Apache-2.0), headed Chromium, dedicated profile, domain allowlist, OBS window capture, stream delay + panic scene | Run the browser on the second notebook (physical isolation) and bring it in over NDI |
| Clips | OBS replay buffer + continuous recording with chapter markers emitted by the agent; scoring = agent markers + chat velocity + audio energy + LLM over transcript; captions from the **TTS script** (aligned), ffmpeg + libass burn-in; render the vertical canvas natively, no AI reframing | Vizard/OpusClip API for A/B comparison only |
| Upload | YouTube Data API (`videos.insert`, now its own 100-call/day bucket), TikTok Content Posting API (needs audit for public posts), Instagram Graph API (Reels) | Twitch Create Clip API for on-platform clips |
| Machines | **One 4090 box is enough for compute** (budget ≈ 15–21 GB VRAM). Use the notebook for the agent's browser sandbox + failover, not for inference | — |

---

## 1. Avatar

### 1.1 Options

| Option | What it is | Programmatic control | Licence / cost | Pros | Cons |
|---|---|---|---|---|---|
| **VTube Studio (Live2D)** | The de-facto Live2D VTuber app (Steam) | WebSocket API on `ws://localhost:8001`: `HotkeyTriggerRequest`, `ExpressionActivationRequest` (fade 0–2 s), `InjectParameterDataRequest` (must be resent ≥ 1×/s in "set" mode, or use "add" mode; `faceFound: true` suppresses the tracking-lost animation), `ParameterCreationRequest` (100 custom params/plugin, 300 global), `ItemLoadRequest` (props), `MoveModelRequest` (per-frame full control), event subscriptions. Hotkey queue 32, a given hotkey max once per 5 frames. **[V]** [DenchiSoft/VTubeStudio](https://github.com/DenchiSoft/VTubeStudio) | Proprietary; free on Steam with watermark, paid DLC removes it **[UNVERIFIED]** | Most mature API of any VTuber app; mic-based lip sync works off a virtual audio cable; big ecosystem (Streamer.bot integration [S](https://docs.streamer.bot/api/sub-actions/integrations/vtube-studio/set-expression-state)) | Needs a Live2D rig (artist time or money); closed app; not usable as a SaaS renderer for the later platform |
| **Live2D in the browser** (Cubism SDK for Web, `pixi-live2d-display`) | Render .moc3 yourself | Full | Cubism SDK release licence: small businesses/individuals (< ¥10M annual sales) exempt **except "Expandable Applications such as avatar systems"** **[V]** [Live2D licence help](https://www.live2d.com/en/?p=586) | Same art as VTS, fully custom | **Platform blocker**: a "create your own character" platform is exactly an avatar system → needs a Live2D licence deal |
| **three-vrm (VRM in browser)** | pixiv's three.js loader/runtime for VRM 0.x/1.0 | Full: expressions (presets incl. `aa ih ou ee oh`, blink, emotions, lookAt), humanoid bones, spring bones, all from TS | MIT **[UNVERIFIED for current version; repo is pixiv/three-vrm]** [releases](https://github.com/pixiv/three-vrm/releases) | Runs as an OBS Browser Source; Claude Code can write/extend it; same renderer later serves the multi-tenant platform; no licence fee | You build idle/gesture animation playback yourself (three.js AnimationMixer + VRMA/Mixamo retarget) |
| **Warudo** | Unity-based 3D VTuber studio, node "blueprints" | Blueprints + C# plugins/Mod SDK; marketing says blueprints can be driven by MIDI, Stream Deck, or **WebSocket messages** [S](https://dekudeals.com/items/warudo); third-party VDeck plugin exposes a WebSocket on 19070 [S](https://booth.pm/en/items/8088698). VRM expressions auto-imported with hotkey blueprint; idle + overlay + transient animations, breathing/sway **[V]** [docs](https://docs.warudo.app/docs/assets/character) | Free on Steam; Warudo Pro tier for commercial/advanced use **[UNVERIFIED — pricing not found]** | Best-looking 3D out of the box, rich idle/animation system, built-in lip sync (reportedly uLipSync-based [S](https://boggy-venom-2c5.notion.site/OVR-Lip-Sync-expressions-335ef6b6bf728037a97eee0663e846bb)) | External control protocol not formally documented; GUI-first; Unity app eats GPU |
| **VNyan** | Free 3D VTuber app (itch.io), node graphs | Node graphs, plugins, WebSocket settings (direction/inbound control unclear) [S](https://github.com/Suvidriel/VNyanDoc/wiki/Plugins) | Free / donation **[S]** | Popular, plugin-friendly | API less documented than VTS |
| **VSeeFace** | Classic VRM tracker/renderer | VMC protocol receive (blendshapes only if names match) [S](https://github.com/emilianavt/VSeeFaceReleases/releases?page=4) | Freeware, closed | Lightweight | Maintenance status in 2026 unclear **[UNVERIFIED]**; tracking-centric |
| **"PNGtuber+" sprite rig** | Layered PNGs (body, skull, jaw, eye-glow, cape) animated in PixiJS/Canvas | Full | Your code | Zero rigging; Leonardo output usable same day; jaw flap + bob + glow + shake reads well for a cartoon villain | Limited expressiveness; looks "cheap" next to Live2D |

Reference open-source AI-VTuber stacks worth reading (not adopting wholesale): **AIRI** (moeru-ai, MIT, Live2D + VRM in a web `stage-ui`, VAD→STT→LLM→TTS, v0.11.x mid-2026 [S](https://www.mintlify.com/moeru-ai/airi)) and **Open-LLM-VTuber** (Live2D, faster-whisper, GPT-SoVITS etc. [S](https://www.sourcepulse.org/projects/1828160)); kimjammer/Neuro (VTS plugin + local STT/TTS [S](https://github.com/kimjammer/Neuro)).

### 1.2 Asset pipelines

**Leonardo (2D) → Live2D**
1. Generate the character on a flat background, front-facing, neutral pose, separated limbs (Leonardo "Elements"/character reference for consistency; API returns a cost breakdown per generation; commercial rights tier-dependent [S](https://apiframe.ai/guides/leonardo-ai-api-guide)).
2. Layer split: **See-through** (SIGGRAPH 2026, single anime image → up to 23 inpainted layers + draw order, exports PSD; ComfyUI wrapper exists) **[V]** [repo](https://github.com/shitagaki-lab/see-through), [ComfyUI node](https://github.com/jtydhr88/ComfyUI-See-through). The authors and a hands-on review call it a *PSD draft generator*, not image-to-Live2D [S](https://lilting.ch/en/articles/see-through-anime-layer-decomposition). It is trained on anime humans — a skull/armoured villain will likely need manual layer fixes **[UNVERIFIED]**.
3. Rig in Cubism Editor (PRO subscription; indie vs business tier switches at ¥10M company revenue [V](https://help.live2d.com/en/?p=2172)). Fully automatic one-click Live2D rigging is **not production-ready** in 2026 [S](https://note.com/dn0288/n/nf89c79d14d27?hl=en).
4. Cost if commissioned: rigging alone ≈ $70–$2,200+ (bust → full body), typical professional half/full body $600–$2,500 incl. art; 20–40 days [S](https://shiralive2d.com/live2d/how-much-does-a-live2d-model-cost/), [S](https://www.shiralive2d.com/live2d-pricing/).

**Tripo (3D) → rigged humanoid → VRM**
1. Generate in Tripo (T-pose/A-pose prompt or image).
2. Tripo API v3: `POST /v3/animations/rig-check` (free compatibility check) → `POST /v3/animations/rig` with `spec: tripo | mixamo` and rig types biped/quadruped/others → `POST /v3/animations/retarget` (presets like `preset:idle`, up to 5 per batch) **[V/S]** [Tripo rig docs](https://developers.tripo3d.com/en/models/rig), [animation docs](https://developers.tripo3d.com/en/models/animation), [ComfyUI node](https://docs.comfy.org/built-in-nodes/TripoRetargetNode). Exports FBX/GLB — **no native VRM export found** [S](https://www.tripo3d.ai/blog/best-vrchat-model-generator-tools).
3. Alternatives for the rig step: Mixamo (bipedal humanoids only; joint rotations copied 1:1 → odd bends on non-human proportions) and Reallusion AccuRIG (humanoid only, tolerates armour/props/missing limbs) [S](https://reallusion.com/FeedBackTracker/Issue/ACCURIG-GREAT-YET-MISSING-SOMETHING), [S](https://community.adobe.com/questions-696/unable-to-map-skeleton-driving-me-insane-589374).
4. Blender + **VRM Add-on for Blender** (import/export VRM, VRM Humanoid armature, MToon) **[V]** [extensions.blender.org](https://extensions.blender.org/add-ons/vrm): map bones to VRM humanoid, author expressions, export VRM 1.0. UniVRM (Unity, MIT) is the alternative exporter.
5. VRoid Studio is irrelevant for this character (it only produces anime humans).

### 1.3 Is a skeleton villain a problem?

- **Auto-rigging:** mostly fine — a skeleton is still a biped. Risks: very thin limbs/ribcage confuse skin-weight auto-assignment (expect manual weight painting on ribs, cape, horns); capes/robes need spring bones or cloth; Mixamo's 1:1 rotation transfer can look wrong on exaggerated proportions [S](https://community.adobe.com/questions-696/blender-script-to-help-with-rig-issues-unnatural-joint-positions-especially-hands-wrists-589765). Run Tripo's free rig-check first.
- **Face:** a skull has **no lips and no face mesh to morph**. VRM 1.0 expressions bind *morph targets, material colours and texture transforms* — not bones **[UNVERIFIED — check VRM 1.0 spec]**. So: pose the jaw bone open in Blender and **save it as a shape key** ("JawOpen"), then bind `aa`/`oh` etc. to fractions of it. Emotions = eye-socket glow colour/intensity via **material colour binds**, brow-ridge shape keys, head tilt. This actually suits a villain: glow intensity becomes the "anger meter".
- **Face tracking:** not needed at all. We drive everything programmatically, which removes the biggest non-human problem (trackers fail on non-human faces). In VTube Studio send `faceFound: true` with injected params so the "tracking lost" idle doesn't trigger **[V]**.

### 1.4 Expressions, idle, gestures from an LLM

Design the avatar as a **state machine with a tiny command vocabulary** that the LLM emits inline or as tool calls, e.g. `{"expr":"smug","gesture":"fist_raise","look":"chat","intensity":0.8}`. The renderer owns timing, blending and idle (breathing, sway, blink, eye-glow flicker, cape spring bones). Never let the LLM set raw bone values.

- VTS: map vocabulary → hotkeys/expressions; continuous values (glow, head tilt) via `InjectParameterDataRequest` at ~30–60 Hz.
- three-vrm: `vrm.expressionManager.setValue()`, AnimationMixer clips (Tripo retarget presets, Mixamo FBX → VRMA retarget) with crossfade; lookAt target = chat panel / browser window / camera.
- Warudo: blueprint per gesture, triggered over WebSocket.

---

## 2. Lip sync & expression from TTS audio

Key point: **we own the audio before it plays.** TTS returns chunks; we can analyse each chunk ahead of playback and send the mouth curve and the audio together, so sync is exact and latency-free. Mic-style real-time analysis is only needed if you route audio through VTS's microphone input.

| Method | How | Real-time? | Licence | Fit |
|---|---|---|---|---|
| **RMS/amplitude → JawOpen** | WebAudio `AnalyserNode` or precomputed envelope per chunk | Yes | — | **Default for a skull** — one jaw parameter is all it has |
| **VTube Studio mic lip sync** | Route TTS output to a virtual cable (VB-CABLE) selected as VTS mic; VTS maps volume + vowel inputs (VoiceA/I/U/E/O, VoiceSilence) to params [S](https://github.com/DenchiSoft/VTubeStudio/wiki/VTS-Model-Settings), [S](https://voxbooster.com/blog/vtuber-voice-changer-setup-complete-guide/) | Yes | VTS | Zero code if using VTS; adds an audio hop |
| **uLipSync** (Unity, MFCC vowel matching) | Calibrated vowel profiles → visemes | Yes | MIT **[UNVERIFIED]** | Warudo reportedly uses it; web ports exist **[UNVERIFIED]** |
| **Rhubarb Lip Sync** | Offline phoneme → 6–9 mouth shapes with timestamps (JSON/TSV) | No — works on utterances of a few seconds [S](https://github.com/DanielSWolf/rhubarb-lip-sync/issues/22) | MIT | Fine per TTS sentence (we have the audio before playback); adds ~0.x–1 s per sentence **[UNVERIFIED]** |
| **OVR LipSync** | 15 visemes from audio | Yes | Meta; **end-of-life**, replaced by Movement SDK [S](https://developers.meta.com/horizon/documentation/unreal/audio-ovrlipsync/) | Avoid for new work |
| **NVIDIA Audio2Face-3D** | Audio → ARKit-52 blendshapes (+tongue/jaw/eyes); v3.0 diffusion ~180M params, 16 kHz input | Yes (60 fps claimed; frame-gap reports on v3.0 streaming [S](https://forums.developer.nvidia.com/t/audio2face-digital-human-sdk-v3-0-diffusion-model-frame-gap-during-streaming-inference-posting-here-as-digital-human-board-is-closed/361109)) | Open-sourced Sept 2025: models under NVIDIA Open Model License (commercial OK), SDK MIT [S], **Audio2Emotion restricted to the Audio2Face project**, NIM container under NVIDIA SLA **[V]** [HF v3.0](https://huggingface.co/nvidia/Audio2Face-3D-v3.0), [paper](https://arxiv.org/abs/2508.16401v1) | Overkill for a skull; interesting later for human-faced platform characters |
| **Viseme timestamps from TTS** | Some cloud TTS (Azure) emit viseme events; local open TTS models generally don't **[UNVERIFIED]** | Yes | — | Not available with the recommended local TTS |

Expression from audio: drive "intensity" (glow, head shake) from the same envelope plus the emotion tag the LLM already chose for the line (IndexTTS-2.x accepts an 8-dim emotion vector [V](https://huggingface.co/indexteam/indextts-2.5) — reuse the same vector for the face).

### 2.1 Local TTS shortlist (input to the VRAM budget; voice deep-dive belongs in a separate doc)

| Model | Size / VRAM | Cloning | Latency | Licence | Notes |
|---|---|---|---|---|---|
| Chatterbox-Turbo (Resemble) | 350M, ~4–8 GB [S] | 5–10 s ref | sub-200 ms claimed, conflicting figures [S](https://findskill.ai/blog/best-open-source-tts-2026/) | MIT [S](https://www.resemble.ai/products/text-to-speech) | Strong default; ONNX build |
| Qwen3-TTS (0.6B / 1.7B Base) | ~4–8 GB [S] | 3 s ref | ~97 ms first packet (paper, tokenizer level) | Apache-2.0 **[V]** [arXiv 2601.15621](https://arxiv.org/html/2601.15621) | 10 languages; streaming |
| IndexTTS-2 / 2.5 | ~6 GB (2.5) **[V]** | yes | — | Bilibili Model License, commercial below thresholds [S] | Emotion vector + speed control; great for villain theatrics |
| CosyVoice 3 | 0.5B, ~4 GB [S] | yes | ~150 ms streaming [S] | Apache-2.0 **[UNVERIFIED]** | — |
| GPT-SoVITS | small | fine-tune | low | MIT **[UNVERIFIED]** | Popular in AI-VTuber projects |
| Kokoro | ~2–3 GB | **no** | very low | Apache-2.0 | Fallback / non-character voice |
| XTTS v2, F5-TTS weights, Fish Audio S2, VibeVoice | — | — | — | **Non-commercial / research** [S](https://ocdevel.com/blog/20250720-tts) | Avoid for a monetised stream/platform |

Legal flag: do **not** clone a real actor's voice (e.g. a Skeletor voice actor). Design an original voice (TTS voice design / RVC pass on your own or a licensed reference).

---

## 3. Broadcasting

### 3.1 OBS + obs-websocket

- **OBS Studio 32.1.x** is current (32.0 Sept 2025: plugin manager, RTX audio effects, hybrid MOV; 32.1.0 Mar 2026: WebRTC simulcast, new audio mixer; 32.1.2 Apr 2026) [S](https://obs-versions.com/blog/obs-studio-32-1-2-release-notes). Licence GPL-2.0.
- **obs-websocket 5.x is built in** (default port 4455 **[UNVERIFIED in this session]**). Requests confirmed in the generated protocol **[V]** [protocol.md](https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md): `SetCurrentProgramScene`, `SetSceneItemEnabled`, `SetInputSettings` (e.g. swap Browser Source URL), `PressInputPropertiesButton` (Browser Source "Refresh"), `TriggerMediaInputAction`, `StartStream`, `SaveReplayBuffer` + event `ReplayBufferSaved{savedReplayPath}`, `GetLastReplayBufferReplay`, `CreateRecordChapter` (chapter markers in recordings), `BroadcastCustomEvent`, `CallVendorRequest` (talk to plugins), and multi-canvas support (`GetCanvasList`, `canvasUuid`, since 5.7.0).
- Browser Sources for: avatar renderer (three-vrm/PixiJS page), chat overlay, "world domination plan" overlay, captions, poll results. All are local web pages served by the orchestrator; push state over a local WebSocket.

Scene plan: `MAIN` (avatar + browser + chat), `BROWSER_FOCUS`, `PLAN_BOARD`, `BRB/TECHNICAL_DIFFICULTIES` (panic scene, also auto-switched by the safety filter), `INTRO/OUTRO`.

### 3.2 Multistream + vertical

| Need | Option | Notes |
|---|---|---|
| Vertical 9:16 canvas | **Aitum Vertical** 1.6.4 (requires OBS 31.1+), second canvas next to the 16:9 one, own outputs; NVENC support added in 1.5.1; Twitch dual-format option (Twitch dual format is **alpha**) [S](https://obsproject.com/forum/resources/aitum-vertical.1715/updates) | Also the cleanest source for Shorts: record the vertical canvas directly |
| Multistream from one PC | **Aitum Multistream** plugin (integrates with Vertical) [S](https://upstream.so/blog/how-to-multistream-on-obs-with-aitum/); or a relay (Restream etc.) to save upload bandwidth | Each extra direct output = extra encode or extra upload |
| Twitch quality ladder | Twitch Enhanced Broadcasting / multi-encode (NVIDIA GPUs) [S](https://alternativeto.net/news/2024/1/twitch-obs-and-nvidia-collaborate-to-bring-multi-encode-streaming-to-twitch) | Uses more NVENC sessions |

Platform rules for simulcasting: Twitch removed exclusivity for Partners/Affiliates in Oct 2023; ToS conditions — Twitch experience no worse than elsewhere, no active redirection off Twitch, merged cross-platform chat on the Twitch video technically barred (enforcement reportedly relaxed in 2026) [S](https://streamer.guide/guides/multistreaming), [S](https://restream.io/blog/twitch-multistreaming-rules-explained/). Kick Partners must toggle "multistreaming" and get reduced payout for that session [S]. **Implication for Overlord:** the avatar should not read out "YouTube chat says…" on Twitch's feed in a way that looks like merged chat — or accept the (low) risk.

AI disclosure: Twitch has no AI-disclosure rule as of Sept 2026 (CEO interview) [S](https://gamesbeat.com/twitch-ceo-dan-clancy-says-platform-doesnt-need-ai-disclosure-rules-for-now/); Kick's March 2026 guidelines require disclosure of AI content that mimics reality [S](https://ag.ny.gov/sites/default/files/social-media-policy-report/2025-q4-kick-streaming-pty-ltd-policy.pdf); YouTube has `status.containsSyntheticMedia` on upload (§6). A cartoon villain that is openly an AI is low-risk, but disclose it in titles/panels anyway.

### 3.3 NVENC on a 4090 alongside AI

- NVENC is a separate fixed-function block (the 4090 has **two** NVENC engines, AV1 capable) [S](https://www.dexerto.com/tech/obs-studio-rtx-4090-av1-encoder-1973089/). Encoding itself barely touches CUDA; there is no benchmark of OBS + local LLM on one consumer GPU [S](https://forums.developer.nvidia.com/t/will-encoding-impact-agx-inference-speed/235847).
- The real contention is **OBS compositing/browser sources (3D engine) vs. CUDA inference** time-slicing on the same GPU → "rendering lag / skipped frames" when TTS/LLM spike. Mitigations: run OBS as Administrator (lets OBS raise its GPU priority) **[UNVERIFIED but widely recommended]**; disable NVENC Look-ahead / Psycho-visual tuning / Max Quality (they use CUDA) [S](https://obsproject.com/forum/threads/%E2%80%9Cnew%E2%80%9D-nvenc-encoding-question.118292/); cap the avatar page at 30–60 fps; keep inference processes at lower priority; watch OBS Stats "frames missed due to rendering lag".
- Session count: 16:9 stream + 9:16 stream + local recording(s) + replay buffer ≈ 3–5 NVENC sessions; GeForce driver limit is 8 concurrent sessions **[UNVERIFIED]**; highest presets can reduce session count [S](https://www.computerbase.de/2022-11/obs-studio-28-1-1-nvidia-nvenc-av1-decoder/).

---

## 4. Chat ingestion & actions

### 4.1 Platform APIs

| Platform | Read chat | Send chat | Other useful surfaces | Gotchas |
|---|---|---|---|---|
| **Twitch** | EventSub `channel.chat.message` v1. WebSocket transport needs a **user** token (`user:read:chat`); Conduits/webhooks with an **app** token need `user:bot` (bot) + `channel:bot` (broadcaster) or mod status **[V]** [announcement](https://discuss.dev.twitch.com/t/available-today-twitch-chat-on-eventsub-an-api-for-sending-chat-and-the-conduit-transport-method-for-eventsub/54596) | Helix Send Chat Message; the **Chat Bot badge** appears only with an app token + `channel:bot`/mod → plan for Conduit to get the badge [V](https://discuss.dev.twitch.com/t/introducing-the-chat-bot-badge-and-chat-identity-for-chatbots/64265). Rate limits: 20 msgs/30 s (non-mod), 100/30 s as broadcaster/mod/VIP, 1 msg/s per channel for non-privileged; overflow → messages ignored for 1 h [V](https://dev.twitch.tv/docs/irc) | Channel points redemptions, Polls, Predictions, raids (`channel.raid`), follows/subs/cheers, Create Clip, Shield Mode, AutoMod events `automod.message.hold` v2 (reason + term) with `moderator:manage:automod` [V](https://dev.twitch.tv/docs/eventsub/eventsub-subscription-types/) | Make the bot account a **mod** of the channel (100/30 s bucket) |
| **YouTube** | `liveChatMessages.streamList` — gRPC server-streaming (`youtube.googleapis.com:443`), resumable via `nextPageToken`; chat ID from `videos.list → liveStreamingDetails.activeLiveChatId` **[V]** [guide](https://developers.google.com/youtube/v3/live/streaming-live-chat) | `liveChatMessages.insert` | Super Chats via message types | **Quota** (10,000 units/day default, no paid tier): `liveChatMessages.list` **5 units/call**, `streamList` 5, `insert` **20**, delete 20 **[V]** [quota table](https://developers.google.com/youtube/v3/determine_quota_cost). Polling `list` every 5 s for 4 h ≈ 2,880 calls ≈ 14,400 units → **over quota**. Use `streamList` (one call per connection/reconnect — billing per streamed response **[UNVERIFIED]**). Sending 100 replies = 2,000 units → reply on YouTube sparingly, speak instead |
| **Kick** | Official public API (launched Feb 2025): **webhook** events `chat.message.sent`, follows, subs, gifts, reward redemptions, livestream status, bans, `kicks.gifted` **[V]** [docs.kick.com](https://docs.kick.com/events/event-types); Ed25519 signatures, at-least-once delivery (dedupe on `Kick-Event-Message-Id`), auto-unsubscribe after 3 failed retries [S](https://github.com/Bukk94/KickLib) | `POST` chat with `chat:write` scope [S](https://docs.rs/kick-api) | Channel rewards | Webhooks need a **public HTTPS endpoint** → Cloudflare Tunnel/ngrok on the stream box; Pusher WebSocket chat is unofficial |

### 4.2 Moderation

Layered: (1) platform-side — Twitch AutoMod level + blocked terms, Shield Mode for raids; YouTube "hold potentially inappropriate"; Kick moderation settings. (2) Own pre-LLM filter — regex/blocklist, slur list, PII patterns, link stripping, a small local toxicity classifier. (3) Output filter on what the villain *says and types* (the model is role-playing a villain — keep "cartoon menace", never real-world violence, real people, hate). (4) **Prompt-injection firewall:** chat is untrusted data; it may influence *what to talk about*, never *what tools run*. Chat never reaches the computer-use agent verbatim — the orchestrator turns a chosen suggestion into a structured, allowlisted task.

### 4.3 Few vs. hundreds of messages

| Load | Strategy |
|---|---|
| 0–5 viewers | Respond to almost every message; fill silence with the agent's own agenda ("phase 3 of the plan"); greet first-time chatters by name |
| 5–50 | Priority queue: paid/channel-point/sub/raid events > first-time chatters > @mentions > rest; max one reply per user per N minutes; batch 10–20 s windows into one LLM call that picks 1–3 to address |
| 50–hundreds | Sliding window (e.g. 15 s) → dedupe + embedding clustering (small local embedding model) → LLM summarises "chat sentiment + top 3 themes + 1 quotable message"; decisions via Twitch Polls API or cross-platform `!vote` tallies; channel-point redemptions as the reliable "chat controls the villain" channel |
| Always | Per-platform rate-limit budgets (Twitch 100/30 s as mod, YouTube quota), speech is the primary output — text replies are rare and cheap |

---

## 5. Computer use on stream

| Tool | How it drives | Fit for stream | Licence |
|---|---|---|---|
| **Playwright MCP** (`claude mcp add playwright npx @playwright/mcp@latest`) | Accessibility-tree actions in a fresh, **headed** Chromium/Firefox/WebKit; no personal logins by default | **Recommended.** Visible window captured by OBS; deterministic; supports an isolated persistent profile; token-heavy on complex pages (50k+ token snapshots) [S](https://www.heyuan110.com/posts/ai/2026-01-28-claude-code-browser-automation/) | Apache-2.0 |
| Playwright CLI as a Claude Code skill | Same engine, shell-driven; Microsoft now prefers it for coding agents [S](https://forum.ui.vision/t/playwright-mcp-vs-claude-in-chrome-vs-ui-vision-mcp-2026/30028) | Good, lower context cost | Apache-2.0 |
| Claude in Chrome | Runs in **your real Chrome with your sign-ins** | **Do not use on stream** — exposes personal sessions | Paid Claude plan |
| Claude computer use (screenshots + mouse/keyboard) | Whole desktop | Slower, less precise for web; only if you need desktop apps | API |

Safety design:
1. **Isolation.** Windows 11 *Home* has no Hyper-V and no Windows Sandbox **[UNVERIFIED — standard Home limitation]** → use a separate local Windows user with no personal data, or VMware Workstation/VirtualBox VM, or (best) run the agent's browser on the **second notebook** and bring it into OBS over NDI (DistroAV plugin) **[UNVERIFIED plugin status]**. No password manager, no saved cards, no personal accounts; the villain gets its own throwaway accounts only where needed.
2. **Network allowlist** (Playwright MCP origin allow/block flags **[UNVERIFIED flag names]**, plus OS-level DNS filter) — start with a curated set of sites (map tools, Wikipedia, image generator, drawing app).
3. **Screen content filter.** Periodic screenshot of the browser source → fast NSFW/gore classifier + a cheap vision-LLM check; on fail, OBS hides the source / switches to the panic scene via websocket. Add an **OBS stream delay** (10–30 s) so the filter and a human kill-switch act before viewers see it.
4. **Audio off** for the agent's browser by default; never play music/video. Copyrighted music → Twitch DMCA strikes and YouTube Content ID blocks/demonetisation of VODs **and clips**. Use a licensed/royalty-free BGM source in OBS only.
5. **Notifications off** (Focus mode), dedicated display/virtual display for the agent window, no file system access beyond a sandbox dir.
6. Action allowlist at the orchestrator level (navigate/click/type within allowed origins; no downloads, no purchases, no form submissions with real data).

---

## 6. Clip automation

### 6.1 Capture

- **Continuous local recording** (Hybrid MP4/MOV) of both canvases, plus `CreateRecordChapter` at every agent-declared moment → chapters become candidate cut points **[V request exists]**.
- **Replay buffer** (e.g. 60–120 s) → `SaveReplayBuffer` on strong triggers; `ReplayBufferSaved` returns the file path **[V]**. Aitum Vertical gives the 9:16 canvas its own record/replay outputs **[UNVERIFIED — confirm in plugin settings]**.
- Because Overlord composes the scene itself, the vertical canvas can be **designed** (avatar top, browser/plan bottom, captions zone) — no AI face-tracking reframe needed.

### 6.2 Highlight detection (combine; each is cheap)

| Signal | Source | Notes |
|---|---|---|
| **Agent-emitted markers** | The LLM tags its own beats ("REVEAL", "RAGE", "chat roasted me") | Strongest and free — Overlord knows when it is being funny |
| Chat velocity / emote bursts | msgs/s z-score, LUL/KEKW/OMEGALUL counts, YouTube Super Chats | Lags the moment by 5–15 s → cut window must start earlier |
| Audio energy | RMS/shout detection on program audio | Villain laugh detection |
| Events | Raids, big redemptions, poll results, browser "success/fail" | From the orchestrator log |
| Transcript + LLM scoring | Claude scores windows on hook strength, self-containedness, payoff | Run post-stream or every N minutes |

Open-source references: OpenShorts (MIT; faster-whisper + PySceneDetect + LLM + MediaPipe reframe) [S](https://themenonlab.blog/blog/openshorts-open-source-ai-clip-generator), OpenClip, Autoclipper (chat-spike signals) [S](https://www.sourcepulse.org/projects/29444144).

### 6.3 Captions & render

- **Overlord's own lines don't need ASR:** the orchestrator already has the exact TTS text and audio durations → generate word timing by forced alignment (WhisperX alignment step / wav2vec2) or from TTS chunk boundaries. Use **faster-whisper large-v3-turbo** (int8 ≈ 1.5 GB, fp16 ≈ 2.5 GB VRAM; tens of × real-time on a 4090 [S](https://whispernotes.app/hi/blog/introducing-whisper-large-v3-turbo), [S](https://gigagpu.com/rtx-4090-24gb-whisper-benchmark/)) only for other audio (raid streamers, browser audio if ever on). faster-whisper MIT; WhisperX BSD **[UNVERIFIED]**.
- Render with **ffmpeg**: cut on keyframes or re-encode with `h264_nvenc`, burn ASS subtitles via libass (word-highlight karaoke style), loudness-normalise (`loudnorm`), add hook title card, 1080×1920, ≤ 60 s for broad compatibility.

### 6.4 Commercial clippers vs self-hosted

| Service | API | Price (reported) | Verdict |
|---|---|---|---|
| OpusClip | REST API, tier gating disputed (Creator vs Business/custom) [S](https://www.heygen.com/blog/opus-pro-alternatives) | $15–29/mo plans, API possibly custom | Benchmark only |
| Vizard | API on Creator tier (~$29/mo, credits per upload minute) [S](https://www.clipspeed.ai/compare/opus-clip-vs-vizard.html) | ~$29/mo | Best API-per-dollar for A/B tests |
| Klap | Reportedly no public API [S] | $29/mo | Skip |
| **Self-hosted** | Your own pipeline | GPU time only | **Recommended** — we have privileged signals (agent markers, TTS script, scene layout) these tools can't see |

### 6.5 Upload APIs

| Platform | API | Limits / rules (2026) |
|---|---|---|
| **YouTube Shorts** | Data API `videos.insert` | **Changed:** `videos.insert` (and `search.list`) now have their **own bucket: 100 calls/day, 1 unit each**; other methods share 10,000 units/day **[V]** [quota page, updated 2026-10-15](https://developers.google.com/youtube/v3/determine_quota_cost) (the old "1,600 units/upload" figure is obsolete; change dates Dec 2025/June 2026 per [S](https://www.blotato.com/blog/youtube-api-pricing)). Uploads from **unverified API projects created after 2020-07-28 are forced private until a compliance audit** [S](https://rapidevelopers.com/api-automations/how-to-automate-youtube-video-uploads-using-the-api). Set `status.containsSyntheticMedia` where applicable [V](https://developers.google.com/youtube/v3/revision_history). YPP "inauthentic content" (mass-produced/repetitive/template) policy, clarified July 2026, is the monetisation risk for auto-clips [S](https://techcrunch.com/2026/07/20/youtube-clarifies-policies-around-ai-slop-and-upsetting-videos/) |
| **TikTok** | Content Posting API (Direct Post) | **Unaudited clients: all posts private (SELF_ONLY), account must be private, max 5 posting users / 24 h**; error `unaudited_client_can_only_post_to_private_accounts`; audit required to post publicly; ~15 posts/day/creator cap shared across API clients; must query `creator_info` before posting **[V]** [Direct Post ref (2026-08-04)](https://developers.tiktok.com/docs/en/content-posting-api-reference-direct-post), [guidelines](https://developers.tiktok.com/docs/en/content-sharing-guidelines). Fallback: "Upload to inbox" draft flow + manual publish, or a third-party audited poster |
| **Instagram Reels** | Graph API (`/media` container → `/media_publish`) | Professional account; **100 API posts / rolling 24 h** (some Meta pages still say 50 — read `content_publishing_limit` live) [V/S](https://developers.facebook.com/documentation/instagram-platform/content-publishing); `instagram_business_content_publish` needs App Review [S] |
| **Twitch clips** | Helix Create Clip (optional `title`, optional `duration` — range not stated), Create Clip From VOD (open beta), Get Clips Download (open beta); scopes `editor:manage:clips` or `channel:manage:clips`; portrait version for dual-format streams [V](https://discuss.dev.twitch.com/t/introducing-clip-api-improvements-and-clip-from-vod-in-open-beta/64492) | Good for on-platform discovery; let Overlord clip itself on Twitch when it detects a moment |

Start the YouTube and TikTok audits **early** — they are calendar-time blockers, not engineering ones.

---

## 7. Compute / VRAM budget on one RTX 4090 (24 GB)

Estimates; measure with `nvidia-smi dmon` and OBS stats during a rehearsal stream.

| Workload | VRAM (approx.) | GPU engine | When |
|---|---|---|---|
| Windows DWM + desktop | 0.5–1 GB | 3D | always |
| OBS: 1080p60 + 1080×1920 canvases, 3–5 browser sources (CEF) | 1–2 GB **[UNVERIFIED]** | 3D | always |
| NVENC: 16:9 stream, 9:16 stream, recording, replay buffer | ~0.1–0.3 GB per session **[UNVERIFIED]** | NVENC ×2 | always |
| Avatar renderer (three-vrm page, 30–60 fps) | 0.3–0.8 GB | 3D | always |
| Agent browser (Playwright Chromium, GPU accel) | 0.3–1 GB | 3D | when used (0 if on notebook) |
| TTS (Chatterbox-Turbo / Qwen3-TTS 1.7B / IndexTTS-2.5) | 4–8 GB | CUDA, bursty | while speaking |
| Optional RVC voice pass | 1–2 GB **[UNVERIFIED]** | CUDA | while speaking |
| Optional local banter LLM (8B Q4_K_M, llama.cpp) | 5–6.5 GB + KV cache; ~90–140 tok/s at 4k ctx [S](https://markaicode.com/benchmarks/llamacpp-qwen-3-rtx-4090-throughput-benchmark/) | CUDA | per reply |
| Whisper large-v3-turbo int8 | 1.5–2.5 GB | CUDA | post-stream or low priority |
| Embeddings + toxicity/NSFW classifiers | 0.5–1 GB | CUDA | periodic |
| **Total (everything resident)** | **≈ 15–21 GB** | | fits in 24 GB with headroom |

Claude Code itself is cloud inference: CPU/RAM only locally. The 7800X3D (8C/16T) and 64 GB RAM are ample for OBS, Node/Python services and ffmpeg (prefer NVENC for clip renders so the CPU stays free for OBS/x264 fallback).

**Do you need a second machine?** Not for compute. VRAM fits; the risk is **GPU time-slicing** causing OBS render lag when TTS/LLM burst, which you mitigate with priorities, a smaller TTS, and rehearsal measurements. The second notebook is worth using for:
1. **Security isolation** of the agent-driven browser (physically separate box, no personal data), fed into OBS via NDI;
2. **Failover** (BRB loop + chat-only mode if the main box stalls);
3. Off-stream jobs (clip rendering, uploads) if you stream long hours.
Requirement: the 4090 box must be *dedicated* during streams (no gaming/dev builds on it).

Latency note for banter: Claude Code headless turns carry ~0.5–1.4 s fixed overhead plus multi-second gaps around tool calls [S](https://agentsroom.dev/de/blog/measure-claude-code-tokens-per-second), [S](https://mer.vin/news/why-claudes-token-stream-goes-quiet-the-moment-it-calls-a-tool/). Use Claude Code for the slow "director" loop (plans, browser tasks, clip judging) and a **fast path** for chat banter (direct Claude API streaming with a small model, or the local 8B LLM), keeping total speech-start latency ≲ 1.5–2 s. Licensing note: Anthropic's help centre says `claude -p`/Agent SDK still draw on subscription limits and Max/Team plans include monthly API credits since 2026-10-07 [V](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan), but the Claude Code terms position subscription login for "ordinary use" and direct product builders to API keys [S](https://autonomee.ai/blog/claude-code-terms-of-service-explained/) — a 24/7 public stream, and certainly the later platform, should run on API keys.

---

## 8. Licence summary

| Component | Licence | Platform-phase concern |
|---|---|---|
| OBS Studio, obs-websocket | GPL-2.0 | Fine to use; don't ship modified binaries without source |
| Aitum Vertical / Multistream | Free plugins (licence **[UNVERIFIED]**) | — |
| three-vrm, UniVRM | MIT **[UNVERIFIED current]** | Good for multi-tenant renderer |
| VRM Add-on for Blender | Open source (MIT **[UNVERIFIED]**) | — |
| Live2D Cubism Editor | Paid subscription (indie/business by revenue) | — |
| Live2D Cubism SDK | Release licence; small-business exemption **excludes avatar systems** **[V]** | **Blocker** for a Live2D creator platform without a deal |
| VTube Studio, Warudo, VNyan, VSeeFace | Proprietary apps | Not embeddable in a SaaS platform |
| Audio2Face-3D | Models: NVIDIA Open Model License; SDK MIT; Audio2Emotion restricted **[V/S]** | OK with care |
| Chatterbox-Turbo / Qwen3-TTS | MIT / Apache-2.0 | OK |
| IndexTTS-2.x | Bilibili Model License (thresholds) [S] | Review before platform scale |
| XTTS v2, F5-TTS weights, Fish S2, VibeVoice | Non-commercial / research | Avoid |
| faster-whisper / WhisperX | MIT / BSD **[UNVERIFIED]** | OK |
| Playwright MCP | Apache-2.0 | OK |
| Tripo, Leonardo outputs | Commercial rights depend on plan tier [S] | Check plan before monetising |

---

## 9. Biggest risks

1. **Character pipeline effort is underestimated.** No tool does Leonardo→Live2D or Tripo→VRM end-to-end; the skeleton's face needs hand-made jaw shape keys and material-glow expressions. Mitigate with a day-1 PNGtuber+ rig and treat the 3D/Live2D model as a parallel track.
2. **Publishing gates are calendar-bound:** TikTok (private-only until audit), YouTube (private-only for unverified projects; YPP inauthentic-content policy), Instagram App Review. Start audits in week 1.
3. **GPU contention on one 4090** → OBS dropped/lagged frames during TTS bursts. Needs a rehearsal with real metrics before going live.
4. **Live-content safety** of an autonomous agent: prompt injection from chat, the browser landing on NSFW/copyrighted material, a villain persona drifting into real-world harm. Needs layered filters, a stream delay, a panic scene and a human kill switch.
5. **Platform API constraints:** YouTube chat quota (5 units per list call, 20 per message), Twitch bot badge requiring Conduit + app token, Kick webhooks needing a public tunnel.
6. **Licensing for the platform phase:** Live2D SDK avatar-system exclusion; non-commercial TTS weights; voice-clone rights; Claude subscription vs API-key terms.
7. **Latency:** Claude Code turn overhead is too slow for snappy banter; needs a fast path.
