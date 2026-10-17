# 00 — Overlord: feasibility and sustainability verdict

Date: 2026-10-17. This is a synthesis of the four research files beside it, which carry the
sources: `01-market-landscape.md`, `02-tech-stack.md`, `03-llm-orchestration.md` and
`04-voice-and-character-assets.md`. Figures marked *est.* are estimates made in those files,
not measurements on this machine.

## Verdict

**Technically feasible on the existing hardware, and sustainable only under five conditions.**
Every component already exists and is commodity: chat in, an LLM, streaming TTS, an avatar with
lip sync, OBS out. The open problems are not technical. They are format (what keeps people
watching week 10), moderation (every AI-stream ban so far was a filter gap), cost and terms (how
the LLM is paid for), IP (the character must be original), and distribution (clip-upload audits
take calendar time).

| # | Condition | Why it matters | Source |
|---|---|---|---|
| 1 | **Run the public stream on an Anthropic API key, not a Pro/Max subscription** | The Consumer Terms bar automated "bot, script" access except through an API key, and bar commercial use. Claude Code via the Agent SDK with an API key is fine. | 03 §3 |
| 2 | **Two lanes: fast chat and slow mind** | Claude Code cannot turn thinking off on the 5.5 models, and adds ~0.5–1.4 s per turn plus tool pauses. Chat banter needs a direct streaming Messages-API call (Haiku 5.5, thinking off; ~0.8–2.5 s to first spoken word *est.*). Claude Code (Agent SDK) is the director: planning, browser use, memory, clips. | 02, 03 §2 |
| 3 | **Moderation on every code path, including fallbacks, and on chat input** | Neuro-sama and Nothing, Forever each took 14-day bans; Nothing, Forever's came from a fallback model that skipped the filter. A villain persona raises the base risk. | 01 §6 |
| 4 | **An original character**, archetype-inspired, not Skeletor | SKELETOR is a registered Mattel trademark and the design is protected. No gold skull on blue body, purple hood, ram staff, name or catchphrases; no cloning of any of the character's voice actors. | 04 §C |
| 5 | **Start the YouTube and TikTok API audits in week one** | Unaudited TikTok apps can post only privately (5 users / 24 h). Unverified YouTube projects upload as private. Clips are the growth channel, so this gates growth. | 02 §6 |

## The runtime on one machine

The RTX 4090 (24 GB), Ryzen 7800X3D and 64 GB RAM carry the whole runtime: TTS, avatar, OBS
NVENC, an optional local fallback LLM and Whisper. That is ~15–21 GB VRAM all at once *est.*;
GPU contention (OBS frame drops during TTS spikes) is the real risk and needs a rehearsal stream.
The second notebook earns its place as the **isolated browser box**: the agent drives a
throwaway browser profile there, and NDI feeds it into OBS. That gives physical separation from
the owner's logins, plus failover.

| Layer | Recommended | Fallback / note |
|---|---|---|
| Mind (slow lane) | Claude Agent SDK (Claude Code engine) on API key; durable state in Overlord's own store via a local MCP server; sessions recycled at segment breaks; nightly memory consolidation on the Batch API | One transcript is never the mind (03 §1) |
| Voice of chat (fast lane) | Messages API, Haiku 5.5 thinking off, prompt-cached persona | Local 26–27B model on the 4090 as an outage fallback |
| TTS | VoxCPM2 compiled (0.11 s first audio, RTF 0.47 measured on this 4090) cloning from designed reference clips; Chatterbox-Turbo (MIT, RTF 0.48) fallback | Qwen3-TTS was the owner's pick by ear but did not stream on Windows pip (~10 s); kp `packages/voice-tts` already wraps engines behind one interface |
| Voice creation | Original voice designed from a text prompt (Qwen3-TTS VoiceDesign / VoxCPM2) → mood reference set → pedalboard post-chain (pitch/formant down, rasp, crypt reverb) | No real-person cloning; a consent ledger if a human actor is ever recorded |
| Avatar | Own three-vrm renderer as an OBS browser source, driven by `{expr, gesture, look, intensity}` commands over WebSocket; jaw shape-key from audio amplitude; eye-socket glow for emotion | Day-1: PNGtuber/2D sprite rig from Leonardo art with jaw frames. Live2D is premium and manual ($600–2,500, and its SDK small-business exemption excludes "avatar systems") |
| Character assets | Leonardo concept → turnaround → Tripo multiview 3D, auto-rig, preset animations (~$1.50/character *est.*) → Blender VRM add-on for jaw/glow expressions | Tripo has no VRM export and likely no face blendshapes |
| Broadcast | OBS 32.x + obs-websocket (scenes, replay-buffer save with path, chapter markers), Aitum Vertical for a 9:16 canvas | Twitch first (most permissive, no AI-disclosure rule); add a disclosure overlay anyway |
| Chat | Twitch EventSub (Conduit later for the bot badge); YouTube via `streamList`; Kick via webhooks | The YouTube chat quota (5 units/read, 20/send, 10k/day) rules out naive polling |
| Computer use on stream | Playwright MCP, visible browser, dedicated profile, domain allowlist, stream delay, panic scene; the agent reads page text, not only pixels | Never Claude in Chrome on stream: it carries real logins |
| Clips | Own pipeline: agent-emitted "moment" markers + chat-velocity spikes + audio energy + LLM transcript scoring → ffmpeg 9:16 reframe, captions from the TTS script → YouTube / TikTok upload APIs | Mass-templated clips risk YouTube's inauthentic-content policy |

## Cost envelope (API-key operation, official prices, prompt-cached; *est.* ±50%)

| Stack | $/hour | 4 h/day × 30 days |
|---|---|---|
| Budget (Haiku chat + light Sonnet director) | ~$2–4.5 | ~$240–540 |
| Quality (Sonnet chat + Sonnet/Opus director) | ~$7.5–9 | ~$900–1,080 |
| Premium (Opus-heavy) | ~$16–20 | ~$1,900–2,400 |

Without prompt caching every figure is 8–12× higher. The API Start/Build tiers stop at
$500/$1,000 a month, so request the Scale tier before the quality stack runs daily.

## What the market says about sustainability

- **Neuro-sama is the only proven AI streamer, and she wins on format, not model:** a human straight
  man, a rival AI (Evil Neuro), collabs, "Filtered" turned into a running joke, 2–3 h streams
  instead of 9, big yearly events, growth through clips. Fans pay to *steer* the content.
- **Agent-on-stream is a real genre with fast novelty decay.** Claude Plays Pokémon peaked at
  2.6k viewers and settled at ~100–200. The visible reasoning and the failures are the content;
  the long arc has to be designed.
- **Gaps nobody fills:** persona plus visible computer use plus a season/story engine; good Twitch
  chat handling in open-source stacks.
- **The platform idea is unproven and legally exposed:** self-serve "create your AI streamer"
  products have little traction, crypto agent-streamers run on token speculation, and
  Character.AI filed Chapter 11 on 2026-10-13 after teen-harm suits (agent-reported, verify).
  Sequence it: **flagship character first, platform second, safety rails from day one.**

## Platform phase: what changes

- Commercial Terms plus API keys; no claude.ai login for end users; reselling needs Anthropic
  approval; "Powered by Claude" branding rules; sales sign-off before per-usage charging.
- Live2D licensing excludes avatar systems, so the platform should own a VRM / sprite pipeline.
- Per-character voice must be designed or consented, with the consent ledger enforced in code.
- Per-tenant moderation policies, kill switches and audit trails become product features.

## Next experiments before building

1. First-token latency benchmark: Haiku 5.5 with the real persona prompt plus VoxCPM2 streaming, end to end on this 4090.
2. 6-hour Agent SDK soak with session recycling and memory-growth measurement.
3. Rehearsal stream with OBS NVENC + TTS + avatar under load: dropped frames and VRAM peak.
4. File the TikTok Content Posting and YouTube API audits.
5. Voice-design session: three original villain voices on a listening board, round-trip intelligibility check.
