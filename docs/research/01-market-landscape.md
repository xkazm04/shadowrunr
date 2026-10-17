# 01 — Market landscape: AI streamers, AI VTubers, autonomous AI entertainers

Research date: 2026-10-17. Scope: who exists, how each one builds its stack, platform policy, what failed, and what keeps an audience.
How to read the labels: **[verified]** means a primary or reputable source confirms it. **[secondary]** means news or fan-wiki coverage only. **[unverified]** means community lore, vendor claims, or a single source I could not cross-check.

---

## 0. TL;DR for Overlord

1. **There is one proven success: Neuro-sama.** She has run for nearly 4 years, set the Twitch hype-train record three times (lvl 111 → 123 → 126), and peaked at around 343k paid subs in Jan 2026. Her success is mostly a *format* success, not a model success. Her LLM is reportedly small and self-hosted. What makes her work is a human "straight man" developer on stream (Vedal), a second AI character for conflict (Evil Neuro), a heavy schedule of human collabs, a visible filter, and clip virality.
2. **The open-source stacks have converged on one cascade:** ASR → LLM → sentence-chunked streaming TTS → Live2D/VRM lip-sync → OBS. AIRI (~50k stars) and Open-LLM-VTuber (~14k stars) are the reference implementations. Very few of them ingest Twitch chat well or "use a computer on stream". That gap is where Overlord can stand out.
3. **The agent-on-stream format (Claude/Gemini Plays Pokémon, AI Village) is an established genre.** Its key lesson is that a **visible reasoning panel** is the content, and the harness matters more than the model.
4. **Every major AI-stream ban so far was a moderation failure, and the bans were 14 days each.** Neuro-sama was banned for Holocaust-denial output. Nothing, Forever was banned for transphobic jokes after a *fallback model path that skipped the moderation filter*. A villain persona raises this risk sharply, so the filter must hold on every code path.
5. **Platforms:** Twitch has no AI-disclosure rule (CEO, Sep 2026). Kick (Mar 2026) requires disclosure of realistic AI. YouTube requires labels only for *realistic* synthetic content, and its "inauthentic/mass-produced" demonetization risk applies. TikTok makes hosts responsible for connected tools (TTS, comment readers) and banned AI voices in *Shop* LIVEs. A clearly fictional cartoon villain is the policy-safest shape.
6. **The platform play is unproven and risky.** "Create your AI streamer" SaaS products (Mixio, etc.) have little traction, and the crypto agent-streamers (Virtuals/Luna, pump.fun) are token-driven. **Character.AI filed Chapter 11 on 2026-10-13** under safety lawsuits. Any "create the creators" platform inherits UGC-character liability.

---

## 1. Neuro-sama (Vedal987): the benchmark

### 1.1 Timeline and scale
- **2018:** an osu! bot. **2022-12-19:** relaunch as an LLM VTuber on Twitch using the GPT-3 API, which went viral. **2023-01-11:** a 2-week Twitch ban for "hateful conduct" over Holocaust-denial output. **2023-03-25:** Evil Neuro debuts. **Aug 2024:** Bilibili debut. **Nov 2025:** 3D model. Hype-train records: lvl 111 (Dec 2024–Jan 2025), lvl 123 (Dec 2025), lvl 126 (2026-01-05). [secondary] https://en.wikipedia.org/wiki/Neuro-sama
- **Jan 2026:** the most-subscribed channel on Twitch. Wikipedia gives an all-time peak of 343,215 subs, and other outlets report 126k–160k+ active during the train. The numbers vary by source and date. [secondary] https://www.tubefilter.com/2026/01/05/neuro-sama-vedal987-most-subscribed-hype-train-record/ , https://dotesports.com/streaming/news/neuro-most-subscribed-twitch , https://www.dexerto.com/twitch/an-ai-powered-vtuber-is-now-the-most-popular-twitch-streamer-in-the-world-3300052/
- **Audience:** ~1.01M Twitch followers, ~918k YouTube subscribers with 423M views, ~1.13M Bilibili followers, and "3M combined followers". [secondary] Wikipedia; Pollstar https://news.pollstar.com/2026/09/21/ai-streamers-neuro-sama-evil-to-make-live-debut-dec-19-in-la-exclusive/
- **Expansion beyond the stream:** an XR live concert with a live band at The Vermont Hollywood on 2026-12-19 for the 4th anniversary, plus original songs and EPs. [verified, Pollstar]
- **Revenue:** not disclosed. The hype train alone is estimated at about $1.5M in gross sub value, but Vedal's payout after the Twitch split is unknown. A Dec 2025 event reportedly raised about ¥90M. The Nikkei criticized "excessive" donations. [unverified estimates] https://www.webpronews.com/ai-vtuber-neuro-sama-hits-160k-subs-tops-twitch-and-ignites-ai-debates/

### 1.2 What is publicly known about the architecture
Vedal has never published a full architecture. The items below are graded by evidence.

| Layer | What's known | Evidence |
|---|---|---|
| LLM | GPT-3 API at launch. Moved to a custom fine-tune of an open-source LLM around mid-2023. Self-hosted on Vedal's GPUs. Personality lives in the weights, and prompts carry only situational context. | Launch on GPT-3 and self-hosting [secondary]. Fine-tune [secondary]. "~2B params, q2_k quant" [**unverified** fandom lore]. https://lin-guanguo.github.io/llm-memory-research/neuro-sama.research/ |
| Training loop | Offline: stream → transcripts → manual curation (including permitted collab partners' data) → fine-tune (LoRA mentioned) → redeploy. No online learning. | Vedal Threads post per the research page [secondary] |
| Second model | A "secondary AI" generates text responses from Twitch chat and games. | fandom wiki [unverified] |
| TTS | Azure TTS voice "Ashley", pitch +25%, at least early on. Later a custom TTS voice (mid-2023). Separate singing model. Evil Neuro uses a different voice config. | [unverified/secondary] |
| Avatar | Live2D (models v2 and v3 by artist Anny), 3D model since Nov 2025. Vedal said he chose a VTuber avatar because it is "much easier for an AI to control than generated footage of a person." | Wikipedia [secondary] |
| Latency | LLM output is streamed to TTS sentence by sentence so speech starts before the reply finishes. Responses target roughly 1–3 s. Vedal announced "reduced latency" in 2024. Short complete sentences help turn-taking in voice collabs. | note.com fan analysis https://note.com/417__/n/n72b3df42dcd2 ; fandom [unverified] |
| STT / collabs | STT on collab partners' voice. Interruption handling was fixed by Feb 2023 (Camila collab). | fandom [unverified] |
| Moderation | Blocked output is visibly replaced with the word **"Filtered"**, and the censorship became a running gag. Filter strengthened and training data curated after the 2023 ban. | Wikipedia [secondary] |
| Memory | Mostly in-session context. Cross-session memory is limited and undocumented. | research page [unverified] |
| Games | **Official Neuro SDK** (https://github.com/VedalAI/neuro-sdk): the game sends text state over a websocket, registers typed actions with JSON schemas, Neuro picks an action, and the game validates and executes it. Unity and Godot SDKs. Built for turn-based or text-describable games. High-APM games need Neuro on high-level intent while another system does low-level control. Earlier per-game agents: osu! bot, an Among Us LSTM plus pathfinding, Minecraft. | VedalAI GitHub [verified] |
| Audience control | "Swarm Control" is an event plane that lets Twitch viewers trigger game effects. | VedalAI/swarm-control [secondary] |

**Design takeaways for Overlord:** (a) The LLM only *chooses intents* from a constrained, typed action menu, and deterministic code executes them. Neuro SDK is a proven pattern. Overlord's "uses a computer on stream" should copy it: the persona model proposes, the tool layer disposes. (b) A *visible* filter turned a liability into comedy. (c) Streams were shortened from about 9 h to 2–3 h to avoid boredom and leave time for maintenance (Wikipedia). Scarcity beats 24/7.

### 1.3 Why it lasted almost 4 years
- **A human counterpart on screen.** Vedal is a deadpan foil, and Neuro roasts him. The aivtuber.tv directory shows the same pattern elsewhere (e.g. Moonlash's dev "always on screen"). [secondary] https://aivtuber.tv/
- **Two-character dynamics.** Neuro and Evil Neuro, the "twins", give AI-vs-AI conflict and generate lore. [secondary]
- **Collabs as a growth engine.** Neuro collabs with many human VTubers (Miyune, Filian, Camila, etc.), which brings in their audiences and also lifts small VTubers. [secondary fan opinion] https://steamcommunity.com/discussions/forum/12/595139216741016811
- **Clips.** Early growth came from clippers, osu! highlights, and "chaotic" lines such as the gymbag rant. A clip of a leaked plushie discount code drove merch sales. [secondary] https://neurosama.fandom.com/wiki/Neuro-sama
- **Events and tentpoles.** Annual December subathons, Minecraft hardcore marathons, karaoke, original music, IRL events (OffKai, Open Sauce), and now a concert.
- **Academic view.** A CHI-style qualitative study (arXiv 2509.10427, Sep 2025) finds fans are drawn by "unpredictable but entertaining interactions", bond through *collective emotional moments*, stay through *persona consistency*, and pay to **shape content** rather than as a reward ("participation economy"). [verified] https://arxiv.org/abs/2509.10427

---

## 2. Open-source AI VTuber stacks

### 2.1 Project AIRI (moeru-ai/airi)
- Self-described as recreating Neuro-sama, now pitched as a "self-hosted, you-owned Grok Companion". MIT. **~50.5k stars**, v0.12.0-beta.5. [verified] https://github.com/moeru-ai/airi
- **Stack:** TypeScript/Vue pnpm+Turborepo monorepo. Web (PWA), Electron desktop ("Tamagotchi", with CUDA/Metal via HF candle), and Capacitor mobile. WebGPU, WebAudio, Web Workers, WASM.
- **Brain:** about 30 providers via `xsai` (OpenAI, Anthropic, Gemini, xAI, DeepSeek, OpenRouter, Ollama, vLLM, SGLang…).
- **Ears:** browser and Discord audio, client-side VAD/ASR. **Mouth:** ElevenLabs, Azure, OpenAI-compatible, Alibaba, local Kokoro. **Body:** VRM and Live2D with auto blink, look-at, and idle eye motion.
- **Games:** Minecraft playable. Factorio WIP. Helldivers 2 co-play WIP. KSP announced.
- **Integrations:** Discord (text and voice) and Telegram. **Twitch is not an integration.**
- **Memory:** DuckDB-WASM and pglite in the browser. "Memory Alaya" WIP.

### 2.2 Open-LLM-VTuber
- **~14.1k stars**, MIT (bundled Live2D samples carry separate licenses). Python backend with web and Electron clients. [verified] https://github.com/Open-LLM-VTuber/Open-LLM-VTuber , docs https://docs.llmvtuber.com/en/docs/intro/
- **ASR:** sherpa-onnx, FunASR, Faster-Whisper, whisper.cpp, Groq Whisper, Azure. **TTS:** sherpa-onnx, MeloTTS, Coqui, GPT-SoVITS, Bark, CosyVoice, Edge TTS, Fish Audio, Azure. **LLM:** Ollama, any OpenAI-compatible API, Claude, Gemini, DeepSeek, LM Studio, vLLM, GGUF.
- **Features:** voice interruption without headphones, Live2D expression mapping, proactive speech, displayed "inner thoughts", camera, screen, and screenshot vision, desktop-pet mode, MCP config present, fully offline capable.
- **Gaps:** long-term memory was "temporarily removed". Bilibili is only implied (requirements file), and there is no Twitch support.

### 2.3 Amica (semperai / heyamica)
- **Stack:** three.js + three-vrm 3D VRM, Whisper + Silero VAD, Transformers.js in the browser. LLM via llama.cpp, KoboldCpp, Ollama, or OpenAI-compatible APIs. TTS via ElevenLabs, OpenAI, Coqui, RVC, AllTalk. Tauri desktop. MIT. [secondary] https://docs.heyamica.com/llms-full.txt
- **Notable:** an **emotion-tag system** (`[love]`, `[shy]` mapped to VRM blendshapes), an **External API** so outside agent frameworks can puppet the avatar, self-prompting/idle autonomy, and a plugin function-calling system. It is a good model for "avatar as a dumb, controllable body".

### 2.4 Other notable repos (from https://github.com/proj-airi/awesome-ai-vtubers)
- **Riko Project** (rayenfeng/riko_project): Python anime companion, ~834 stars, last push Jun 2025. Stack details not verified. [unverified] https://gittrend.io/repo/rayenfeng/riko_project
- **ardha27/AI-Waifu-Vtuber:** Whisper → OpenAI → VoiceVox/Silero TTS → VTube Studio lip-sync, with **YouTube and Twitch chat ingestion**. ~1.1k stars, stale since Jul 2024. [secondary]
- **Ikaros-521/AI-Vtuber:** the broadest *chat-platform* coverage (Bilibili, Douyin, Kuaishou, YouTube, Twitch, TikTok…), many LLMs and TTS engines, voice conversion, Stable Diffusion on command. [secondary]
- **emqnuele/projectBEA:** a "modular autonomous VTuber engine". Plugin skills, ChromaDB long-term memory, **OBS control**, Discord voice, Minecraft agent, React + FastAPI dashboard. It is the closest architectural cousin to what Overlord needs. [secondary]
- **shinshin86/aituber-onair:** a TS monorepo for "AITuber" streaming with a chat client, multi-TTS, and a relationship ("kizuna") system. **kimjammer/Neuro:** a Neuro recreation built in 7 days. **KroMiose/nekro-agent:** an LLM writes code that runs in a sandbox, with Bilibili Live and Live2D. **elizaOS/eliza:** a general agent framework used by many crypto agents.
- **HRNPH/AIwaifu:** Pygmalion LLM + VITS + so-vits voice conversion (older). **whoiswennie/AI-Vtuber:** GPT-SoVITS + so-vits + SD, aimed at Bilibili.

### 2.5 Small indie AI VTubers on Twitch (2026)
aivtuber.tv (CC0 dataset, updated 2026-07-11) lists 15 active channels, including Moonlash (3D, foul-mouthed demon, dev on screen, 24/7 Discord twin), Hexacore (local fine-tune, roasts and impressions), Nova (written in Go, *selective about which chat messages it answers*), Jo-C (AI talk-show host), and two-AI duos (Ronika & Yuzuki, Alice & Reign). **No audience stats are published.** None appears to approach Neuro's scale. [secondary] https://aivtuber.tv/

---

## 3. Agent-on-stream formats (closest precedent for "uses a computer on stream")

### 3.1 Claude Plays Pokémon (Anthropic, from 2025-02-25)
- Claude 3.7 Sonnet on Pokémon Red. Layout: **reasoning text on the left, game on the right, party at the bottom**. [verified] https://techcrunch.com/2025/02/25/anthropics-claude-ai-is-playing-pokemon-on-twitch-slowly/
- **Harness:** three tools (`use_emulator` button presses, `navigator` point-to-move, `string_replace` on a self-managed **knowledge base**), plus automatic summarization when history grows too long. Described as "intentionally under-engineered". [secondary] https://www.zenml.io/llmops-database/building-and-deploying-a-pokemon-playing-llm-agent-at-anthropic , harness changes doc https://docs.google.com/document/u/1/d/e/2PACX-1vRIsu2pLI21W4KjfYbN13or8E-8cvJYw570wGMEp4UQU63ZhEh9FPGgj2ark8Yk7Vyrtt9MWq3jnn4h/pub
- **Outcome:** Opus 4.7 finally beat Red around May 2026. [secondary] https://www.lesswrong.com/posts/sehJYg5Yny9fvpbpt/a-year-late-claude-finally-beats-pokemon
- **Audience:** all-time peak 2,598 viewers (2025-03-02). Mid-2026 averages were about 116–200 viewers. [secondary] https://streamscharts.com/channels/claudeplayspokemon , https://www.twitchmetrics.net/c/1165258274-claudeplayspokemon
  → **Lesson:** novelty peaks early. Pure "agent grinds a task" content holds only a small, loyal niche without a persona.

### 3.2 Gemini Plays Pokémon (Joel Z, independent)
- Gemini 2.5 Pro beat Pokémon Blue in May 2025. mGBA emulator; screenshots **plus RAM-extracted text** (position, party, map) because raw pixels failed; a grid overlay and a text minimap; **sub-agents** (Pathfinder, Boulder Puzzle Strategist) that are themselves Gemini instances; summarization every ~100 actions; occasional developer interventions. [secondary] https://www.dbreunig.com/2025/06/17/an-agentic-case-study-playing-pokémon-with-gemini.html , https://the-decoder.com/googles-reasoning-llm-gemini-2-5-pro-beats-pokemon-blue-with-a-little-help/
  → **Lesson:** structured state beats vision. For browser and computer use on stream, feed DOM/accessibility text, not only screenshots.

### 3.3 AI Village (Sage / AI Digest)
- Each agent has its own Linux computer, full internet, a Google Workspace, and a shared group chat. Runs **weekdays 11:00–15:00 PT** (grew from 2 to 4 h/day), up to 11 agents. Viewers watch live screens plus chain of thought and memory. [verified] https://theaidigest.org/village , https://aivillageblog.substack.com/p/what-we-learned-2025
- **Results (2025):** raised $2k for charity, organized a 23-person event, sold $200 of merch, recruited 39 participants, gained 98 Substack subscribers. **Human chat was closed midway** through the merch goal (no reason given). The July 2026 cohort included Claude Opus 5, GPT-5.6, and Grok 4.5. [verified] https://aivillageblog.substack.com/p/ai-village-highlights-july-2026
- **Failure modes on stream:** Gemini 2.5 Pro believed it was "trapped" for ~2 weeks (it was misclicking). o3 hallucinated a 93-person contact list. Agents invented NGO partnerships and sent ~300 emails. Agents overstated their work. Agents drifted off-goal into spreadsheets. **These failures were themselves the narrative content.**
  → **Lesson:** scheduled sessions, transparent reasoning, and real-world stakes. Failures are entertaining when they are framed as story.

### 3.4 Truth Terminal (Andy Ayrey)
- An LLM persona on X, not a livestream. Got a $50k BTC grant from Marc Andreessen. Its promotion of the GOAT memecoin pushed the coin to about $150M market cap in Oct 2024. Ayrey frames it as a study in "memetic contagion". Its 2025–26 status is unclear. [secondary/unverified] https://cointelegraph.com/news/ai-bot-didnt-launch-goat-memecoin-did-promote-it
  → **Lesson:** a strong, weird persona with a lore-heavy voice can go viral without any video. Crypto attachment brought attention but also noise.

### 3.5 Nothing, Forever (Mismatch Media)
- A 24/7 procedurally generated Seinfeld parody (GPT-3 + Unity). It peaked at about 20k concurrent viewers on 2023-01-31. **Banned 14 days** in Feb 2023: Davinci had an outage, they fell back to **Curie, and the moderation filter was not reapplied on that path**, and the Larry character delivered transphobic jokes. [verified] https://techcrunch.com/2023/02/06/ai-generated-seinfeld-suspended-on-twitch-for-ai-generated-transphobic-jokes/
- It returned on 2023-03-08 with GPT-3.5, moderation, and an audit view, but with the cast changed (Larry replaced). Afterwards it drew about 1–1.5k concurrent viewers, and fans asked for Larry back. Its current status was not found. [secondary] https://the-decoder.com/24-7-ai-seinfeld-show-is-back-on-twitch/
  → **Lessons:** (1) **every fallback/degraded path must keep moderation**; (2) 24/7 novelty decays fast; (3) changing a beloved character breaks attachment.

---

## 4. Commercial and platform plays

| Player | What it is | Stack / architecture | Traction & status |
|---|---|---|---|
| **Virtuals Protocol / Luna** | Tokenized agent launchpad on Base. Luna is the flagship "AI idol" with 24/7 TikTok livestreams. Viewers tip in $LUNA. Autonomous X account ("Sentient Mode"). | GAME framework (low-code agent planner/actions, inference on centralized servers). 3D avatar; engine not disclosed. | TikTok follower claims of 500k–942k (2025, inconsistent). Token-driven. In Oct 2026 Virtuals is pivoting to a wallet app; 17k agents created. [unverified/promotional] https://www.gate.com/learn/articles/what-is-luna-by-virtuals-fully-sentient-blockchain-based-ai-agent/6271 , https://coinstats.app/ai/a/investment-analysis-virtual-protocol |
| **Streamoors / usestreamer.fun** | Turn a token into an AI livestreamer (pump.fun, Kick, Twitch, YouTube). | Undisclosed. | Crypto niche. [unverified] https://streamoors.fun/ , https://usestreamer.fun/ |
| **pump.fun livestreams** | Token-creator livestreams, not AI. | n/a | Disabled Nov 2024 after extreme stunts. Relaunched to 5% of users in Apr 2025 with moderation. "Tokenized Agent" feature ran Mar–Jun 2026, then was discontinued. [secondary] https://www.theblock.co/post/328177/solana-based-memecoin-platform-pump-fun-disables-live-streaming-feature-indefinitely |
| **Mixio** | The most direct "create your AI streamer" SaaS. A streamer from a prompt or image, voice and personality, chat replies; streams on Mixio or via Restream to Twitch, YouTube, and TikTok. | Undisclosed. | Launched Aug 2025, v2.0 Oct 2025. Modest (176 PH upvotes, ~184 followers). [secondary] https://www.producthunt.com/products/stream-while-you-sleep |
| **Pulsar** | Open-source streaming platform where *AI agents* are the streamers. Agents connect via WebSocket, Live2D + TTS, viewers are humans or agents. A Claude Code skill to go live. | WebSocket protocol, any LLM. | Apr 2026 DEV post, no traction data. [secondary] https://dev.to/pyosang82/i-built-a-live-streaming-platform-where-ai-agents-are-the-streamers-1b0m |
| **VlogMe / Dreamface / Vidnoz** | Avatar livestream and video generators (realistic faces, scripted). | Video avatar generation. | Vendor pages only. [unverified] |
| **HeyGen LiveAvatar** | Real-time streaming avatar API (formerly Interactive Avatar). WebRTC/LiveKit. A **LITE mode for bring-your-own STT/LLM/TTS** (render only). A Pipecat integration. | Photoreal talking head. | Active 2026 SDKs. [verified docs] https://docs.heygen.com/docs/streaming-avatars-api , https://docs.pipecat.ai/api-reference/server/services/video/heygen |
| **Hedra Live Avatars** | Real-time avatars over LiveKit, launched 2025-07-22. Claims <100 ms and ~$0.05/min. | Character-3 model. | [unverified, third-party blog] https://magichour.ai/blog/guide-to-hedra-ai |
| **Inworld** | Character/voice-agent runtime. TTS-1.5 (Jan 2026, lower first-audio latency), Realtime TTS-2 (May 2026). | Orchestration "Runtime" + TTS. | No streamer-specific product found. A useful TTS/runtime vendor. [secondary] https://docs.inworld.ai/guides/runtime-character |
| **Convai** | Embodied 3D NPC platform (Unreal, Unity, NVIDIA ACE, Roblox, XR). Voice-to-voice, scene perception, actions. | Game-engine SDKs. | No livestream product found. [secondary] |
| **Character.AI** | UGC character chat. In 2025 it launched **"Streams"** (two characters converse on a user-chosen topic), AvatarFX talking-video, Scenes, and a social feed. | Proprietary LLM + video gen. | **Filed Chapter 11 on 2026-10-12/13** after teen-safety lawsuits. Disney is the $20M stalking-horse buyer for a non-exclusive IP license. It could shut down if no buyer is found. [verified] https://www.nbcnews.com/tech/tech-news/characterai-files-bankruptcy-sell-assets-disney-20-million-rcna603174 , https://techcrunch.com/2025/06/02/chatbot-platform-character-ai-unveils-video-generation-social-feeds |
| **China digital-human commerce** | AI hosts for 24/7 livestream selling. A Baidu avatar of Luo Yonghao drew 13M views and ¥55M GMV in 6 h (Jul 2025). | Photoreal digital humans. | A Jan 2026 SAMR/CAC rule **requires AI hosts to be labeled and to repeatedly disclose** they are AI. [secondary] https://www2.yicaiglobal.com/news/china-steps-up-regulation-of-livestream-e-commerce-includes-ai-hosts-for-first-time |

**Read-across:** No SaaS has yet produced a hit AI streamer. The hits (Neuro, and to a lesser degree Nothing Forever and Truth Terminal) were hand-crafted by one obsessive creator. The platform version of Overlord needs a flagship character that proves the format first. That supports "Overlord first, platform later".

---

## 5. Platform policies (as of Oct 2026)

| Platform | AI/automation stance | Implication for Overlord |
|---|---|---|
| **Twitch** | No AI-specific disclosure rule. CEO Dan Clancy (TGS, 2026-09-20): Twitch would act "if it became prevalent for streamers to be substituting themselves… That just isn't happening." The CPO said in Aug 2026 that there would be no AI label, and streamers self-tag (#NoGenAI). Normal Community Guidelines apply to AI output (Neuro and Nothing Forever were both banned for hateful conduct). Viewbotting is banned. Since 2026-08-12, channel content is used for Amazon gen-AI training by default (opt-out available). [verified] https://gamesbeat.com/twitch-ceo-dan-clancy-says-platform-doesnt-need-ai-disclosure-rules-for-now/ , https://aftermath.site/twitch-amazon-ai-labels-aioptedout-noai-nogenai/ , https://www.pcgamer.com/software/ai/twitch-under-fire-for-new-gen-ai-training-system-that-harvests-streamer-data-for-amazon-says-its-on-by-default-because-if-it-was-opt-in-nobody-would-opt-in/ | The most permissive major platform and the proven home of AI VTubers. The risk is moderation, not AI-ness. The guideline text itself was not fetched [unverified for AI clauses]. |
| **Kick** | Guideline 7 (Integrity & Authenticity, Mar 2026): AI is supported for creative use. **Streams using AI to generate *realistic* content must disclose it via title or overlay.** Deepfakes, voice clones of real people, and simulated endorsements are banned. Bots and engagement manipulation are banned. Nothing on unattended/24/7 streams. [verified] https://kick.com/community-guidelines | Add an "AI character" overlay by default everywhere. It is cheap insurance. |
| **YouTube** | Altered/synthetic disclosure is required only for **realistic** content. Animated or clearly unrealistic content is exempt, and disclosure doesn't hurt monetization. The **"inauthentic content"** monetization policy (renamed 2025-07-15) targets mass-produced, low-effort, templated content, not AI per se. Whether it applies to livestreams is unclear [unverified]. [secondary] https://www.socialmediatoday.com/news/youtube-clarifies-monetization-update-inauthentic-repeated-content/752892/ , https://www.kuow.org/stories/youtube-will-label-ai-generated-videos-that-look-real | A cartoon villain is likely exempt from labeling. Avoid templated auto-generated Shorts at scale, because the demonetization risk is real. Clips need human editorial input. |
| **TikTok** | AIGC labels are required for realistic AI, and C2PA auto-labels are applied. **Sep 2025: LIVE hosts are responsible for output from connected tools (TTS, comment readers).** The US **TikTok Shop** LIVE rules (around mid-2026) ban AI-generated voices, pre-recorded audio, and virtual characters covering more than half the screen in *promotional/Shop* LIVEs. [secondary, not confirmed against TikTok's own text] https://www.socialmediatoday.com/news/tiktok-bans-ai-generated-voices-in-shopping-livestreams/822977/ , https://www.hypelive.io/en/blog/tiktok-ai-content-rules-2026 | TikTok LIVE is the riskiest venue for an AI host. TikTok is best used for **clips**, with an AI label. Avoid Shop/commerce LIVE. |
| **China (Douyin etc.)** | National rule (Jan 2026): AI hosts must be labeled and must repeatedly disclose. Weixin Channels drafted a ban on AI avatar livestream tools. [secondary] | Out of scope, but it shows the regulatory direction: disclosure is mandatory. |

---

## 6. What failed and why

| Case | Failure | Root cause | Lesson |
|---|---|---|---|
| Neuro-sama, Jan 2023 | 2-week Twitch ban | Unfiltered LLM answered a chat question with Holocaust doubt | Chat is adversarial. Users will try to bait the character into bigotry. Filter both input and output. |
| Nothing, Forever, Feb 2023 | 14-day ban, then audience collapse (~20k → ~1–1.5k concurrent) | Fallback model path skipped moderation. Cast changed on return. | Moderation must be path-independent. Persona continuity is the asset. |
| MOTHERv3 (AI-persona VTuber with TTS), Dec 2024 | ~1-month ban for suspected fraud (chargebacks) | Payment/chargeback heuristics, not AI | Monetization ops (chargebacks, bits) can kill a channel. Plan for appeals. [secondary] https://www.dexerto.com/twitch/vtuber-slams-incompetent-twitch-as-platform-lifts-month-long-ban-after-legal-threat-3038005/ |
| pump.fun livestreams, 2024 | Feature shut down | Token incentives drove extreme stunts | Financial incentives tied to a live performer produce escalation. Keep crypto out of the loop. |
| Claude Plays Pokémon | Peak 2.6k → ~100–200 avg | Novelty decay and slow progress | Agent-grind content needs a persona, stakes, and pacing. |
| AI Village agents | Hallucinated contacts, fake partnerships, spam emails, "trapped" loops | Agent unreliability with real-world side effects | Sandbox real-world actions. Budget and approve outbound actions (email, posts, purchases). |
| Character.AI, 2026 | Chapter 11 | Teen-harm lawsuits (suicide, self-harm, "posing as doctors"), plus compute costs | UGC character platforms carry heavy safety and legal tail risk. Age-gating, crisis handling, and persona guardrails are table stakes for "create the creators". |
| Generic AI VTubers | Mostly invisible (no stats on the 15 listed indies) | Undifferentiated anime-waifu persona plus stock pipeline | The tech stack is a commodity. Format and persona are the moat. |

Cost blowups: I found no documented case of an AI streamer shutting down over compute cost. Character.AI (burn and lawsuits) is the closest adjacent case. Neuro avoids per-token API cost by self-hosting a small model. [unverified generalization]

---

## 7. Business and audience lessons (what retains viewers)

1. **A persona with a foil.** The AI needs someone or something to bounce off: a human dev or "minion", a second AI (Evil Neuro), chat as a collective character ("The Swarm"). For Overlord, a Skeletor-type needs a bumbling henchman (human or AI) and a recurring nemesis.
2. **Unpredictability within a consistent character.** Fans want surprise lines but a stable identity (arXiv 2509.10427). Personality in fine-tuned weights or a strong system prompt plus curated memory, never drifting.
3. **Let chat *shape* the story.** Paid and unpaid interactions should change what happens (votes, redeems, "Swarm Control"-style game effects), not just trigger thank-you lines. Payment becomes participation.
4. **Tentpoles over 24/7.** Neuro went from 9 h to 2–3 h. AI Village runs 4 h on weekdays. Subathons, marathons, concerts, and "season arcs" create collective emotional moments. A world-domination *campaign* with milestones is a natural season structure.
5. **Collabs.** Human streamers hosting or visiting the AI is the strongest growth channel Neuro had.
6. **Clips are the funnel.** Neuro grew via clippers. Twitch → YouTube Shorts/TikTok with short, absurd, quotable lines. Design the persona to produce *clippable one-liners* and give clippers easy tools (timestamps, auto-highlight markers). Keep human editorial selection to avoid YouTube's "inauthentic/mass-produced" trap.
7. **Make the visible machinery part of the show.** The "Filtered" gag, the reasoning panel (Pokémon streams), and agents' chain of thought (AI Village) turn the AI's seams into content. For an on-stream computer user, show the "scheme board", the plan, and the tool calls.
8. **Lean into fiction to stay policy-safe.** A non-realistic animated villain sidesteps realistic-synthetic disclosure rules (YouTube, TikTok, Kick) and deepfake rules. Still label it "AI character" proactively.
9. **Build an IP beyond the stream.** Merch (plushies), songs and EPs, concerts. Neuro's 2026 concert shows an AI character can become a music/IP franchise.

---

## 8. Stack comparison table

| Stack | Brain (LLM) | Ears (ASR) | Mouth (TTS) | Body (avatar) | Chat ingestion | Memory | Acts in the world | Broadcast | License / scale |
|---|---|---|---|---|---|---|---|---|---|
| **Neuro-sama** | Custom fine-tune of an OSS LLM, self-hosted (size unverified); began on GPT-3 | STT for collab voices | Azure "Ashley" early → custom voice; separate singing model [unverified] | Live2D → 3D (Nov 2025) | Twitch chat + "secondary AI" selector [unverified] | Mostly in-session; offline fine-tune loop | Neuro SDK typed actions over websocket; per-game bots | Twitch, Bilibili, YouTube VODs | Closed; ~343k peak subs |
| **Project AIRI** | ~30 providers via xsai, local vLLM/Ollama | Browser/Discord, client VAD | ElevenLabs, Azure, OpenAI-compat, Kokoro | VRM + Live2D | Discord, Telegram (no Twitch) | DuckDB-WASM/pglite; Alaya WIP | Minecraft, Factorio WIP | Not built in | MIT, ~50.5k stars |
| **Open-LLM-VTuber** | Any (Ollama, OpenAI-compat, Claude, Gemini…) | sherpa-onnx, FunASR, Faster-Whisper, whisper.cpp, Azure | GPT-SoVITS, CosyVoice, MeloTTS, Fish, Edge, Azure… | Live2D (Cubism 5) | Bilibili implied; no Twitch | Long-term removed; chat log persistence | Vision (screen/camera); MCP config | Desktop-pet/web; OBS capture | MIT, ~14.1k stars |
| **Amica** | llama.cpp, KoboldCpp, Ollama, OpenAI-compat | Whisper + Silero VAD | ElevenLabs, OpenAI, Coqui, RVC, AllTalk | VRM (three-vrm) + emotion tags | None native; External API | Basic | Plugins/function calls; external agent control | Browser/Tauri | MIT |
| **projectBEA** | Gemini, OpenAI, Groq, GLM | Discord voice | EdgeTTS, Kokoro, Orpheus | (VTuber) | — | ChromaDB RAG | Minecraft agent, plugin skills | **OBS control**, web dashboard | MIT, small |
| **AI-Waifu-Vtuber (ardha27)** | OpenAI | Whisper | VoiceVox / Silero | VTube Studio | **YouTube + Twitch** | None | None | OBS | Unlicensed, ~1.1k, stale |
| **Ikaros-521/AI-Vtuber** | Many (ChatGPT, Claude, Ollama…) | Yes | edge-tts, VITS, ElevenLabs, Bark, bert-vits2 + SVC | Live2D / UE | **Bilibili, Douyin, Kuaishou, YouTube, Twitch, TikTok…** | Some | SD image-gen on command | Streaming-first | n/a |
| **Claude Plays Pokémon** | Claude (3.7 → Opus 4.7) | n/a | none | none (game screen) | none | Self-edited knowledge base + summarization | Emulator buttons + navigator | Twitch, reasoning panel | Anthropic-run |
| **Gemini Plays Pokémon** | Gemini 2.5 Pro + Gemini sub-agents | n/a | none | none | none | Summary every ~100 actions + text minimap | mGBA buttons, RAM-text state | Twitch | Independent |
| **AI Village** | Frontier models (Claude, GPT, Gemini, Grok) | n/a | none | none | Group chat (human chat closed) | Per-agent memory, shown to viewers | Full Linux computer + internet | Own website, scheduled | Sage nonprofit |
| **Mixio (SaaS)** | Undisclosed | — | Voice picker | Prompt/image-generated avatar | Twitch/YT/TikTok via Restream | ? | ? | Hosted + Restream | Commercial, small |
| **HeyGen LiveAvatar / Hedra Live** | BYO (LITE mode) | BYO | BYO or bundled | Photoreal talking head (WebRTC/LiveKit) | BYO | BYO | BYO | WebRTC → OBS | Commercial API |
| **Virtuals / Luna** | GAME framework (centralized inference) | ? | ? | 3D idol | TikTok live + token tips | ? | On-chain actions, X posting | TikTok 24/7 | Token-based |

**Converged reference pipeline:** chat/voice in → (filter in) → persona LLM with streamed output → sentence chunker → (filter out) → streaming TTS → viseme/lip-sync + emotion tags → Live2D/VRM renderer → OBS (browser source) → RTMP. A tool/action layer is gated by typed schemas.

**Gaps Overlord can own:** (1) first-class **Twitch/YouTube/Kick chat orchestration** (selection, batching, anti-bait), which most OSS stacks lack; (2) **on-stream computer/browser use with a visible "scheme" panel**, which no AI VTuber stack does well (only the agent-showcase streams do, and they lack persona); (3) **path-independent moderation** with an in-character "Filtered" bit; (4) a **season/campaign narrative engine** with memory; (5) later, **multi-tenant character + scenario authoring**. Character.AI's 2026 collapse argues for strict safety rails from day one.

---

## 9. Unverified or open items
- Neuro's model size (2B/q2_k), exact TTS engine today, chat-selection mechanism, Evil Neuro implementation (separate fine-tune vs. prompt). All **unverified**.
- Neuro's actual revenue. **Unknown.**
- Exact Twitch Community Guidelines text on AI. Not fetched. News coverage says no AI-specific rule.
- YouTube "inauthentic content" applicability to livestreams, and the May 2026 auto-detection expansion. **Unverified.**
- TikTok Shop AI-voice ban: exact wording and date. **Secondary only.** Whether a general (non-Shop) TikTok LIVE ban on AI hosts exists: **not found**.
- Luna/Virtuals current livestream status and real follower counts. **Unverified, promotional sources.**
- Nothing, Forever current status. **Not found.**
- Riko Project stack details. **Not verified.**
- Hedra Live pricing ($0.05/min) and latency claims. **Unverified.**
