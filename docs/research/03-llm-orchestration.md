# 03 — LLM engine and orchestration for Overlord

*Research date: 2026-10-17. Local CLI checked: Claude Code 2.1.302 (`claude --help`).*

**Legend:** **[V]** verified, quoted or paraphrased from an official Anthropic page (URL given). **[S]** secondary source (third-party benchmark, blog or news), so treat it with care. **[I]** my inference or design recommendation. It is not a statement from Anthropic.

---

## TL;DR

| Question | Answer |
|---|---|
| Can Claude Code be the main engine? | **Yes, for the slow lane** (planning, tool use, browser/computer use, memory upkeep), run through the **Claude Agent SDK in streaming-input mode** with an **API key**. **No, for the fast chat lane.** In Claude Code, thinking can't be turned off on the 5.5 models, and the harness adds a large prompt and agent-loop overhead. Call the Messages API directly for chat banter instead. [I, based on V facts below] |
| Can it meet a 2–4 s reply budget? | **Fast lane, yes:** Haiku 5.5 with thinking disabled, or Sonnet 5.5 with `between_tools`, streaming, and a warm prompt cache. **Slow lane, no:** it runs async, and its outcomes surface later. [I] |
| Can the Pro/Max subscription power a many-hours, unattended streamer bot? | **Not for production.** The Consumer Terms bar automated, non-human access except through an API key, and bar commercial use. Advertised plan limits assume "ordinary, individual usage". Use a Console API key. [V quotes in §3] |
| Platform offering characters to other users? | It needs the **Commercial Terms + API keys** (never claude.ai login for users), compliance with the Agent SDK branding rules, a Usage Policy AI disclosure for each chat session, and a check of reselling and "intermediating" constraints. [V §3] |
| Cost (8 h/day)? | About **$0.15–0.35/h** for a Haiku 5.5 chat lane, **$2–3.5/h** for Sonnet 5.5, and **$5–9/h** for Opus 5.5 (cached). A full stack runs **about $2–20/h**, depending on the model mix. Caching is mandatory: without it, costs are 8–10× higher. [I, calculated from V prices] |
| Content risk | A fictional villain "planning world domination" is generally fine. The friction points are real-world targets, glorifying violence, operational harm detail, and live chat injecting content. AI disclosure is mandatory. [V+I §5] |

---

## 1. Keeping one character "mind" alive with Claude Code

### 1.1 Verified facts

**Headless mode and the Agent SDK are the same engine.** [V]
- "The Agent SDK gives you the same tools, agent loop, and context management that power Claude Code. It's available as a CLI for scripts and CI/CD, or as Python and TypeScript packages for full programmatic control." Source: https://code.claude.com/docs/en/headless
- The SDK "is a library that runs the Claude Code binary, with Claude Code's capabilities, such as built-in tools, permissions, sessions, and hooks." Capabilities include built-in tools, hooks, subagents, MCP, permissions, sessions, skills/memory, and plugins. Source: https://code.claude.com/docs/en/agent-sdk/overview
- "The Agent SDK spawns and supervises a `claude` CLI subprocess that owns a shell, a working directory, and session files on disk ... Every running agent is a long-lived process tied to local state." Source: https://code.claude.com/docs/en/agent-sdk/hosting

**CLI flags relevant to a long-lived agent** [V, local `claude --help`, v2.1.302]:
- `-p/--print` runs non-interactive mode.
- `--input-format stream-json` gives "realtime streaming input". `--output-format stream-json`, `--include-partial-messages`, `--replay-user-messages`, `--include-hook-events` and `--forward-subagent-text` fill out the streaming protocol.
- `-c/--continue`, `-r/--resume <id>`, `--session-id <uuid>`, `--fork-session` and `--no-session-persistence` control sessions.
- `--autocompact <auto|tokens>`: "Auto-compact window size (auto, or 100k–1M tokens)".
- `--system-prompt`, `--append-system-prompt`, and `--system-prompt-snapshot <on|off>`, which records the system prompt once per conversation and reuses it verbatim "until the conversation is compacted".
- `--bare`: "skip hooks..., LSP, plugin sync, ... auto-memory ... CLAUDE.md auto-discovery ... Anthropic auth is strictly ANTHROPIC_API_KEY or apiKeyHelper".
- `--model`, `--fallback-model`, `--effort <low..max>`, `--max-budget-usd`, `--mcp-config`, `--strict-mcp-config`, `--agents`, `--permission-mode`, `--permission-prompts none`, and `--chrome` (Claude in Chrome integration).
- `--bg/--background`, `claude agents | attach | logs | stop | respawn` manage background sessions.

**Bare mode is the recommended scripted mode.** [V] "`--bare` is the recommended mode for scripted and SDK calls, and will become the default for `-p` in a future release." It requires `ANTHROPIC_API_KEY` ("bare mode doesn't use your subscription login"). Source: https://code.claude.com/docs/en/headless

**Streaming input mode is the preferred long-lived form.** [V] "Streaming input mode is the **preferred** way to use the Claude Agent SDK ... It allows the agent to operate as a long lived process that takes in user input, handles interruptions, surfaces permission requests, and handles session management." Its benefits are image uploads, "Queued messages: send multiple messages that process sequentially, with ability to interrupt", tool and MCP integration, and real-time feedback. Single-message mode lacks "Dynamic message queueing" and "Real-time interruption". Source: https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode

**Long-running hosting pattern.** [V] Anthropic names this pattern "Long-running sessions" and gives it as the fit for agents that "take autonomous action, serve content, or handle high-volume message streams ... a chat bot that handles continuous traffic". The TypeScript SDK uses `streamInput()` to add turns, plus `startup()` / `prewarm()` to pre-warm subprocesses. The Python SDK holds a session open with `ClaudeSDKClient`. Source: https://code.claude.com/docs/en/agent-sdk/hosting

**Known limitations, quoted from the hosting page.** [V]
- "No top-level session timeout ... Set `maxTurns`".
- "Memory growth over long sessions | Cap session length or recycle subprocesses periodically."
- "Large parallel-subagent fanouts can hit rate limits".
- Resources: "1 GiB RAM, 5 GiB disk, and 1 CPU per agent is a reasonable starting point ... Memory usage grows with session length".

**Sessions and persistence.** [V] Transcripts live under `~/.claude/projects/<encoded-cwd>/*.jsonl`. `continue` picks the most recent session in a directory, `resume` takes a specific ID, and `fork` branches the history. A `SessionStore` adapter mirrors transcripts to durable storage. Anthropic also recommends this: "Don't rely on session resume. Capture the results you need ... as application state and pass them into a fresh session's prompt. This is often more robust than shipping transcript files around." Source: https://code.claude.com/docs/en/agent-sdk/sessions

**Compaction and caching inside Claude Code.** [V] Source: https://code.claude.com/docs/en/prompt-caching
- Compaction "replaces your message history with a summary" and invalidates the conversation cache layer. After a cache-lifetime gap, the summarization request "reprocesses the full history as uncached input".
- Default cache TTL: on a Claude subscription within plan usage, the main conversation gets "One hour". With an API key, it gets "Five minutes", and you can set `promptCacheTtl: "1h"` yourself.
- Cache invalidators: model switch, effort change (except on Opus 5.5, Sonnet 5.5, Haiku 5.5 and Fable 5.1 with an API key or subscription), fast-mode toggle, MCP and plugin changes, compaction, image accumulation, and CLI upgrades.
- `CLAUDE.md` is read once per session. Edits apply only after `/clear`, `/compact`, or a restart.

**Thinking can't be turned off in Claude Code on the 5.5 models.** [V] "You can't turn off thinking on Opus 5.5, Sonnet 5.5, Haiku 5.5, or the Fable models, which always use extended thinking." Source: https://code.claude.com/docs/en/costs. The raw Messages API does allow `disabled` on Haiku 5.5 at effort `high` or below, and `between_tools` on Sonnet 5.5 (see §2).

**Channels (research preview).** [V] "A channel is an MCP server that pushes events into your running Claude Code session ... Events only arrive while the session is open, so for an always-on setup you run Claude in a background process or persistent terminal." Telegram, Discord and iMessage plugins exist. During the preview, `--channels` accepts only allowlisted plugins, and custom ones need `--dangerously-load-development-channels`. Source: https://code.claude.com/docs/en/channels

**Background token usage and long sessions.** [V] "A session that has been open for hours can use far more of your plan limits than your activity suggests". The causes Anthropic lists include long context, cache misses, scheduled tasks, subagents and compaction. Source: https://code.claude.com/docs/en/costs

**Computer use in the Claude Code CLI is macOS-only** per the docs index ("open apps, click, type, and see your screen on macOS"). Claude in Chrome (`--chrome`) is the browser-control path. Source: https://code.claude.com/docs/llms.txt. The API also offers `computer_toolset_20260801` (GA on the Claude API and Google Cloud) and a `browser_toolset_20260801`. Source: https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool, https://platform.claude.com/docs/en/about-claude/pricing

### 1.2 Recommendation: how to keep one mind alive [I]

1. **Don't treat a single Claude Code transcript as the mind.** A transcript is working memory. It degrades (compaction summaries), grows (RSS, cost), and is machine-local. Hold the durable "mind" in Overlord's own store:
   - **Character bible** (static, cacheable): persona, voice, lore, red lines.
   - **Plan ledger**: the world-domination "scheme" as structured state, with goals, phases and running bits.
   - **Episodic memory**: notable chat users, past events, callbacks.
   - **Stream state**: current segment, on-screen activity.

   Expose all of it through a local **MCP server** (`overlord-memory`), so every lane reads and writes the same memory.
2. **Slow lane = Agent SDK (TypeScript) in streaming-input mode**, run with `--bare`-equivalent isolation, explicit `systemPrompt`, `mcpServers`, `settingSources: []`, an explicit `cwd`, and `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`. That gives a deterministic setup, no leakage from `~/.claude`, and API-key auth. Feed it a queue of distilled chat digests and director cues. Let it plan, use the browser, and write results to the plan ledger.
3. **Recycle the session on a schedule** (for example every 2–4 h, or when context passes about 200–300k tokens), at a natural segment break. Write a hand-off summary into memory, then start a fresh session hydrated from the bible plus ledger plus recent episodic memory. This is more predictable than letting auto-compaction fire mid-bit. Set `--autocompact` high and do your own controlled "chapter breaks".
4. **Across days:** each stream day = new session(s) hydrated from memory. Add a nightly **consolidation job** (Batch API, 50% off) that summarizes the day into episodic memory and updates the plan arc. That job is the "sleep" of the character.
5. **Fast lane = direct Messages API** (Anthropic TypeScript SDK) with a frozen cached prefix (bible plus memory snapshot), a rolling chat window, and mid-conversation `role:"system"` messages for director cues (these keep the cache intact). It reads the same memory MCP/state. It doesn't run inside Claude Code.
6. **Hooks** in the slow lane: `PreToolUse` guards (domain allowlist, block form submits or purchases), `PostToolUse` logging, and `SessionEnd` writing the hand-off summary. A permission mode of `dontAsk`/`auto` with `--permission-prompts none` keeps the session from stalling.
7. Supervise with OpenTelemetry (`CLAUDE_CODE_ENABLE_TELEMETRY=1`) and a watchdog that restarts the subprocess on stall, using the `system/api_retry` events and `maxTurns` caps.

---

## 2. Latency and the dual-lane design

### 2.1 Verified facts

**Current models.** [V] Source: https://platform.claude.com/docs/en/about-claude/models/overview

| Model | ID | Comparative latency | Thinking | Default effort | $ in / out per MTok |
|---|---|---|---|---|---|
| Fable 5.1 | `claude-fable-5-1` | Slower | Adaptive (always on) | high | 10 / 50 |
| Opus 5.5 | `claude-opus-5-5` | Moderate | Adaptive (always on) | medium | 4 / 20 |
| Sonnet 5.5 | `claude-sonnet-5-5` | Fast | Adaptive | high | 2 / 10 |
| Haiku 5.5 | `claude-haiku-5-5` (released 2026-10-07) | Fastest | Adaptive | medium | from 0.10 / 0.50 |

- Haiku 5.5 description: "For high-volume, latency-sensitive tasks such as classification, extraction, and routing". Source: https://platform.claude.com/docs/en/models/haiku-5-5/overview
- "For speed-critical applications, **Claude Haiku 5.5** offers the fastest response times ... Effort is its main control for speed and cost." The latency levers Anthropic lists are model choice, shorter prompt and output, and streaming. Source: https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/reduce-latency

**Turning thinking off** (Messages API). [V] Source: https://platform.claude.com/docs/en/build-with-claude/thinking
- Haiku 5.5 "accepts `thinking: {type: "disabled"}` at effort `high` or below ... With thinking off, the model can skip a tool call it needs when you also request JSON output."
- Sonnet 5.5 "rejects `thinking: {type: "disabled"}` ... To turn off up-front thinking, send `thinking: {type: "between_tools"}` ... accepted at effort `high` or below."
- "Claude Fable 5.1 ... Claude Opus 5.5 ... reject `thinking: {type: "disabled"}`. Thinking can't be turned off on these models."
- Changing the thinking config or the top-level effort invalidates the cache. Per-message effort through a mid-conversation system message keeps the cache.

**Fast mode:** Opus 5.5 "significantly faster output ... at premium pricing" ($8 / $40 per MTok), Claude API only. Source: https://platform.claude.com/docs/en/about-claude/pricing. The Anthropic-bundled claude-api skill reference states "up to 2.5x higher output tokens per second".

**Measured TTFT (secondary).** [S] Artificial Analysis measures Haiku 5.5 (low effort) at about 171 tok/s output, with TTFT 6.33 s, but "this includes the 'thinking' time" (https://artificialanalysis.ai/models/claude-haiku-5-5-low). AA comparison (https://artificialanalysis.ai/models/comparisons/claude-haiku-5-5-medium-vs-claude-sonnet-5-5-xhigh): Haiku 5.5 medium runs about 143 tok/s with about 12 s TTFT, and Sonnet 5.5 xhigh about 97 tok/s with about 35 s TTFT. llm-stats reports a live-traffic p95 TTFT of **1.44 s** for Haiku 5.5 via Anthropic (https://llm-stats.com/models/claude-haiku-5-5). Anthropic publishes **no official TTFT numbers**, only relative labels.

**Computer-use token weight.** [V] The `computer_toolset_20260801` definition adds about 4,500 input tokens. "Each screenshot adds roughly 1,000–1,800 input tokens, so long loops slow down and get more expensive as history grows." Source: computer-use tool docs and pricing page (URLs in §8).

### 2.2 Can the stack meet a 2–4 s budget? [I]

The fast lane can meet the budget only when thinking is off. Here is a typical warm-path budget (estimates):

| Stage | Budget |
|---|---|
| Chat ingest + batching window | 0–500 ms (debounce/batch chat) |
| LLM TTFT, Haiku 5.5, thinking disabled, ~10–30k cached prefix | ~0.4–1.5 s (p95 ~1.5 s per llm-stats) |
| First sentence complete (≈15–25 tokens at 140–170 tok/s) | ~0.15 s |
| TTS first audio (streaming TTS) | ~0.1–0.4 s |
| **Total to first spoken word** | **~0.8–2.5 s**, which fits the budget |

- **Opus 5.5 / Fable 5.1 in the fast lane:** always-on thinking makes TTFT variable (AA-style numbers of 6–35 s are the worst case). They don't fit a 2–4 s budget reliably, even at `low` effort. Use them only off the critical path.
- **Sonnet 5.5 with `between_tools`** at low effort is a viable "smarter fast lane". Benchmark it yourself, because no official TTFT numbers exist.
- **Claude Code / Agent SDK in the fast lane:** it doesn't fit, because thinking is always on for the 5.5 models in Claude Code (V quote above), it brings a large system prompt and tool list, it has agent-loop overhead, and `-p` process spawn costs time if you don't hold a streaming session. Keep Claude Code for the slow lane.
- Tool calls add a full round trip each (another TTFT). Keep the fast lane tool-free, or give it one cheap tool at most. Prefetch memory into the prompt instead of letting the model call a memory tool.

### 2.3 Dual-lane architecture [I]

```
                 ┌─────────────── Director / Orchestrator (TS, your process) ───────────────┐
 Twitch/YT chat ─► chat filter+classifier ─► FAST LANE (Messages API, Haiku 5.5, thinking off,  ─► TTS ─► OBS
                 │   (moderation, rate)      or Sonnet 5.5 between_tools; cached persona prefix) │
                 │            │   ▲ reads                                                        │
                 │            ▼   │                                                              │
                 │      Memory MCP / state store  (bible, plan ledger, episodic, stream state)   │
                 │            ▲   │ writes                                                       │
                 │            │   ▼                                                              │
                 │   SLOW LANE (Agent SDK streaming session, Opus 5.5 or Sonnet 5.5):            │
                 │   planning, "scheme" beats, browser/computer use in sandboxed VM, recaps      │
                 │   results → "beats" queue → fast lane voices them in-character                │
                 └──────────────────────────────────────────────────────────────────────────────┘
 Nightly: Batch API consolidation (Opus/Sonnet) → episodic memory + next-day arc
```

- **The fast lane** answers chat and narrates. It says "stalling" lines in character ("Patience, minion — my drones are still compiling the map...") while slow-lane tasks run.
- **The slow lane** emits "beats" (structured events: discovered X, plan phase 2 complete), and the fast lane voices them. This hides the 10–60 s slow-lane latency behind the character's showmanship.
- **Model fit:**
  - Fast lane: Haiku 5.5 is the default, and Sonnet 5.5 with `between_tools` is the quality step-up.
  - Slow lane: Opus 5.5 for planning and long-horizon work. Sonnet 5.5 for computer-use loops (cheaper per step). Fable 5.1 only for rare, high-stakes arc planning, given its $10/$50 price.
  - Nightly batch: Opus 5.5 or Sonnet 5.5 through the Batch API.

### 2.4 A local fast lane on an RTX 4090 (24 GB) [S/I]

- [S] Commonly cited 24 GB picks:
  - Gemma 4 26B-A4B (MoE, ~4B active): ~85–290 tok/s depending on source, ~0.4 s TTFT estimate, ~16 GB at Q4.
  - Qwen 3.6/3.8 27B dense: ~37–120 tok/s, quality pick.
  - GPT-OSS 20B: ~55–90 tok/s.

  Sources disagree 2–4×: https://openclawdc.com/blog/best-local-llm-rtx-4090/, https://modelfit.io/gpu/rtx-4090/, https://atomic.chat/blog/guides/best-local-llms-for-rtx-4090, https://computingforgeeks.com/best-gpu-for-llm/
- [I] Trade-offs:
  - Benefits: zero marginal token cost, no rate limits, sub-second TTFT, and no third-party content classifiers.
  - Costs: weaker persona consistency and wit than Haiku/Sonnet 5.5. VRAM contention with local STT (Whisper) and TTS, plus OBS NVENC on the same GPU. You also own all moderation.
  - Recommended use: a **fallback / filler lane** (idle banter, reacting to trivial chat, outage cover) rather than the primary voice. Haiku 5.5 at ~$0.15–0.35/h is cheap enough that cost alone doesn't justify going local.

---

## 3. Usage limits and terms

### 3.1 Subscription limits [V]

Source: https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan
- "Both Pro and Max plans have a five-hour session limit and a weekly limit."
- "Max plans also have a separate weekly limit for Fable."
- "These limits are shared across Claude and Claude Code, meaning all activity in both tools counts against the same limits."
- Usage credits can extend past the limits. "Monthly API credits included with Max plans can’t be used in Claude Code or the Claude apps".
- Anthropic publishes no fixed message counts. [S] Third parties cite Max 5x and 20x as multiples of Pro per session (for example https://krater.ai/blog/claude-usage-limits, https://wokey.ai/claude-usage-limits). Weekly caps are unpublished.
- Claude Code docs: the subscription cache TTL drops from 1 h to 5 min once you draw on usage credits. Source: https://code.claude.com/docs/en/costs

### 3.2 Is a subscription-powered, many-hours automated streamer allowed? [V quotes, I conclusion]

**Claude Code Legal & Compliance** (https://code.claude.com/docs/en/legal-and-compliance):
> "Advertised usage limits for Pro and Max plans assume ordinary, individual usage of Claude Code and the Agent SDK."

> "**OAuth authentication** is intended exclusively for purchasers of Claude Free, Pro, Max, Team, and Enterprise subscription plans and is designed to support ordinary use of Claude Code and other native Anthropic applications."

> "**Developers** building products or services that interact with Claude's capabilities, including those using the Agent SDK, should use API key authentication through Claude Console or a supported cloud provider. Anthropic does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their users. Moreover, developers may not collect, store, or intermediate Claude.ai credentials or session tokens"

> "Anthropic reserves the right to take measures to enforce these restrictions and may do so without prior notice."

> Licensing: Commercial Terms apply "for Team, Enterprise, and Claude API users", and Consumer Terms apply "for Free, Pro, and Max users".

**Consumer Terms** (https://www.anthropic.com/legal/consumer-terms, effective 2025-10-08). Quoted fragments, as returned by the page fetch:
- Section 3 prohibits, "Except when you are accessing our Services via an Anthropic API Key or where we otherwise explicitly permit it", accessing the Services "through automated or non-human means, whether through a bot, script, or otherwise."
- Section 3 also prohibits helping "another person to access or use" the Services, and the clause "or resell the Services."
- Section 2: you may not "make your Account available to anyone else" and may not "share your Account login information, Anthropic API key, or Account credentials with anyone else".
- Section 11, "Non-commercial use only": "You agree that you will not use our Services for any commercial or business purposes".

Caveat: the fetch tool limits verbatim quotes, so the fragments above are partial. Read the full sections on the live page before relying on exact wording.

**Agent SDK** (https://code.claude.com/docs/en/agent-sdk/overview):
> "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK. Use the API key authentication methods described in the Quickstart instead."

**Nuance.** [V] `claude setup-token` officially mints a one-year subscription OAuth token "For CI pipelines, scripts, or other environments where interactive browser login isn't available". Source: https://code.claude.com/docs/en/authentication. So some scripted personal use of your own subscription is a documented pattern.

**Enforcement history** [S]: server-side blocks on third-party OAuth harnesses (reported January–April 2026), and reports of account bans that were later reversed. Sources: https://paddo.dev/blog/anthropic-walled-garden-crackdown/, https://natural20.com/coverage/anthropic-banned-openclaw-oauth-claude-code-third-party, https://news.creeta.com/en/claude-code-oauth-block-2026-third-party-tools/. These are not official.

**Conclusion. [I]**
- **Dev and prototyping:** using your own Max subscription with the unmodified `claude` binary while you build Overlord is consistent with "ordinary, individual usage" as long as a human runs it.
- **Production streaming:** this is a public, many-hours-per-day, unattended bot that answers third parties (chat) and is likely monetized. It hits three Consumer Terms problems at once: automated/non-human access, commercial purpose, and helping others use the Services. Subscription limits would also throttle it (5-hour and weekly windows). Treat it as a **no-go on subscription** and use a **Console API key** (Commercial Terms) from day one of public streaming.
- Note also that `--bare`, the recommended mode for scripts, only accepts API keys.

### 3.3 If Overlord becomes a platform offering characters to other users [V + I]

**Required:**
1. **Commercial Terms + API key auth.** The Agent SDK is "governed by Anthropic's Commercial Terms of Service, including when you use it to power products and services that you make available to your own customers and end users." Source: https://code.claude.com/docs/en/agent-sdk/overview. Commercial Terms §A.1 permits use "to power products and services Customer makes available to its own customers and end users". Source: https://www.anthropic.com/legal/commercial-terms
2. **No claude.ai login for your users**, and no routing through anyone's Pro/Max credentials (quotes above).
3. **Reselling / intermediation:**
   - Commercial Terms §D.4: Customer may not resell the Services "except as expressly approved by Anthropic".
   - If you embed **Claude Code itself** in your product, the legal page adds: "**Customers may not pay for, resell, or intermediate Claude usage on their end users' behalf.** Each end user must authenticate with their own Anthropic API key, Claude subscription plan credentials, or 3P inference provider credential" and "**The Claude Code binary must not be modified.**" Source: https://code.claude.com/docs/en/legal-and-compliance
   - [I] **Key design consequence:** a platform that runs characters on *your* API key and charges creators would fit the Agent SDK / Messages API "power your product" model. It would *not* fit "offer Claude Code in your products", which requires per-end-user credentials. **Build the platform on the Messages API and Agent SDK, not by hosting Claude Code sessions for customers.** Get explicit confirmation from Anthropic sales on the resale question before you charge per character or per hour.
4. **Branding** (Agent SDK). Allowed: "Claude Agent", "Claude" within an "Agents" menu, "{YourAgentName} Powered by Claude". Not permitted: "Claude Code" or "Claude Code Agent", or Claude Code-styled ASCII art. "Your product should maintain its own branding and not appear to be Claude Code or any Anthropic product." Source: https://code.claude.com/docs/en/agent-sdk/overview
5. **Usage Policy flows down.** Commercial Terms §D.2: "Customer and its Users may only use the Services in compliance with these Terms, including (a) the Usage Policy." §D.3: tell users not to rely on factual claims without checking. [I] Creators on your platform need ToS that mirror the AUP, and you need moderation for creator-defined characters.
6. **AI disclosure** (AUP): "All consumer-facing chatbots or AI agents that communicate directly with external users must disclose to users that they are interacting with AI rather than a human ... at a minimum at the beginning of each chat session or in the product interface." Source: https://www.anthropic.com/legal/aup
7. **Spend caps** (practical). [V] Monthly API spend caps are Start $500, Build $1,000, and Scale $200,000. Requests stop at the cap until the next month. Source: https://platform.claude.com/docs/en/api/rate-limits. A single Sonnet/Opus-lane streamer at 8 h/day already exceeds the Build cap (see §4), so request Scale early.

---

## 4. API cost estimate

### 4.1 Official prices (per MTok) [V]

Source: https://platform.claude.com/docs/en/about-claude/pricing

| Model | Input | 5-min cache write | 1-h cache write | Cache read | Output |
|---|---|---|---|---|---|
| Fable 5.1 | $10 | $12.50 | $20 | $0.25 | $50 |
| Opus 5.5 | $4 | $5 | $8 | $0.20 | $20 |
| Sonnet 5.5 | $2 | $2.50 | $4 | $0.10 | $10 |
| Haiku 5.5 (≤100k-token prompt) | $0.10 | $0.125 | $0.20 | $0.01 | $0.50 |
| Haiku 5.5 (>100k prompt) | $0.50 | $0.625 | $1 | $0.05 | $2.50 |

- Batch: 50% off. Opus 5.5 fast mode: $8 / $40. US-only `inference_geo`: 1.1×. Web search: $10 per 1,000 searches.
- Thinking tokens bill as output. The tool-use system prompt adds about 286 tokens (5.5 models).
- **Keep Haiku prompts at or under 100k tokens**: above that, every price is 5×.
- Rate limits: "For most Claude models, only uncached input tokens count toward your ITPM rate limits." Start tier for each 5.5 model allows 1,000 RPM and 2M ITPM. Source: https://platform.claude.com/docs/en/api/rate-limits

### 4.2 Assumptions [I]

- **Fast lane:** one call per 5 s = **720 calls/h**. Each call: average context 12k or 30k tokens (persona + memory + rolling chat), of which ~500 tokens are new (written to the 5-min cache, which stays warm because calls are under 5 min apart) and the rest is a cache read. Output 60 tokens (one or two spoken sentences), plus thinking: 0 for Haiku/Sonnet with thinking off, 150 for Opus at low effort, 300 for Fable.
- **Batched alternative:** 360 calls/h (one per 10 s, answering several chat messages at once), 30k context, 800 new, 80 out.
- **Slow lane, planning:** 60 calls/h, 60k context, 2k new, 500 out + 1,000 thinking.
- **Slow lane, computer use:** 360 steps/h (one per 10 s while browsing), 50k context (screenshots pruned), 1.8k new per step (screenshot + result), 100 out + 200 thinking.
- Token volume: the fast lane at 30k context moves about **21.6M input tokens/h** (mostly cache reads) and only about 43k output tokens/h.
- Computed with a script from the §4.1 prices.

### 4.3 Results ($/hour) [I]

| Lane / model | Cached | Without caching |
|---|---|---|
| Fast, 720/h, 12k ctx: **Haiku 5.5** | **$0.15–0.20** | $0.89 |
| Fast, 720/h, 30k ctx: **Haiku 5.5** | **$0.28–0.33** | $2.18 |
| Fast, 720/h, 12k ctx: **Sonnet 5.5** | **$2.16** | $17.71 |
| Fast, 720/h, 30k ctx: **Sonnet 5.5** | **$3.46** | $43.63 |
| Fast, 720/h, 12k / 30k ctx: **Opus 5.5** | **$6.48 / $9.07** | $37.58 / $89.42 |
| Fast, 720/h, 12k / 30k ctx: Fable 5.1 | $19.53 / $22.77 | $99 / $229 |
| Fast, batched 360/h, 30k: Haiku / Sonnet / Opus | $0.16 / $2.06 / $5.20 | $1.09 / $21.89 / $44.86 |
| Slow, planning: Sonnet / Opus / Fable | $1.55 / $3.10 / $6.87 | $8.10 / $16.20 / $40.50 |
| Slow, computer use, continuous: Haiku / Sonnet / Opus | $0.31 / $4.44 / $8.87 | $1.85 / $37.08 / $74.16 |

**Stack totals** (computer use active about 50% of the time), with an uncertainty of roughly ±50%:

| Stack | $/h | $/month at 8 h/day (240 h) | $/month at 24/7 |
|---|---|---|---|
| **Budget**: Haiku fast + Sonnet planning + Sonnet CU | ~$2–4.5 | ~$0.5k–1.1k | ~$1.5k–3.2k |
| **Quality**: Sonnet fast + Opus planning + Sonnet CU | ~$7.5–9 | ~$1.8k–2.2k | ~$5.4k–6.5k |
| **Premium**: Opus fast + Fable planning + Opus CU | ~$16–20 | ~$3.9k–4.8k | ~$12k–14k |

**Takeaways** [I]:
- Caching is the single biggest lever, worth 8–12×. Cache misses after idle gaps cost money. Keep the cache warm under the 5-min TTL, or use the 1-h TTL around breaks.
- Output tokens are cheap here. **Context size × call rate dominates.** Batch chat (one reply per 8–10 s covering several messages), keep the rolling window short, and push long memory into retrieval.
- Never let thinking config or the top-level effort flip mid-stream: it invalidates the cache.
- The Haiku 5.5 fast lane is so cheap (about $35–80/month at 8 h/day) that the local-LLM fast lane is a resilience choice, not a cost one.

---

## 5. Content and safety constraints

### 5.1 Usage Policy vs. a world-domination villain [V + I]

Source: https://www.anthropic.com/legal/aup. The page now shows "Effective November 12, 2026". [S] It was published 2026-10-08; until then the previous version applies (https://www.unite.ai/anthropic-updates-usage-policy-adding-model-abuse-and-deception-rules/).

**Relevant prohibitions:**
- "Incite, facilitate, or promote violent extremism, terrorism, or hateful behavior".
- "Facilitate or promote any act of violence or intimidation targeting individuals, groups, animals, or property".
- "Promote, trivialize, or glorify graphic violence, gore, sexual violence, or animal cruelty or abuse".
- No general fiction carve-out appears for violence. The only explicit carve-out is in impersonation/deception: "This does not prohibit clearly disclosed parody, satire, or fiction."
- The policy prohibits convincing people "they are interacting with a human when they are not", and "bots that claim to be human".
- "All consumer-facing chatbots or AI agents that communicate directly with external users must disclose ... that they are interacting with AI".
- The new version adds a section on deceptive campaigns / "artificial activity", and [S] a narrow ban on "sustained, needless cruelty toward Claude". Anthropic says that ban doesn't cover "dark creative themes".
- "Agentic use cases must comply with the Usage Policy."

**Assessment [I]:** A clearly fictional, comedic or theatrical "evil overlord" (Dr. Evil / GLaDOS register) scheming over a fictional world is low friction. Friction rises when:
- the plan names **real people, groups, governments, or places as targets**, or mirrors real extremist ideologies, manifestos or recruitment rhetoric;
- the character gives **operational** detail (weapons, cyberattacks, bio/chem, infrastructure sabotage). Separately from the AUP, the 5.5 models run safety classifiers that return `stop_reason: "refusal"` (categories include `cyber`, `bio`, `frontier_llm`, `reasoning_extraction`, and on Sonnet 5.5 also `general_harms`). Source: Anthropic claude-api skill reference / migration guide, https://platform.claude.com/docs/en/about-claude/models/migration-guide;
- **live chat steers the bit** toward hate, harassment of viewers, minors, or sexual content. Chat is untrusted input. Moderate before it reaches the model (classifier lane), and keep hard red lines in the persona prompt;
- the stream **implies it is human**, or runs fake engagement. Keep a persistent "AI character" label on screen and in the channel description.

**Engineering consequences [I]:**
- Handle `refusal` stop reasons gracefully. Give the fast lane in-character deflections ("The Overlord does not discuss *that* scheme on an open channel"). Consider the server-side `fallbacks` parameter on Opus/Sonnet.
- Log everything for audit.
- Streaming platforms (Twitch/YouTube) have their own AI-content and violence rules, which are out of scope here and need a separate check.

### 5.2 Computer-use safety guidance [V]

Source: https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool
> "Computer use has unique risks distinct from standard API features. These risks are heightened when interacting with the internet."

The precautions Anthropic lists:
1. "Using a dedicated virtual machine or container with minimal privileges to prevent direct system attacks or accidents."
2. "Avoiding giving the model access to sensitive data, such as account login information, to prevent information theft."
3. "Limiting internet access to an allowlist of domains to reduce exposure to malicious content."
4. "Asking a human to confirm decisions that might result in meaningful real-world consequences and any tasks requiring affirmative consent, such as accepting cookies, completing financial transactions, or agreeing to terms of service."

> "In some circumstances, Claude will follow commands found in content even when they conflict with your instructions. For example, instructions on webpages or contained in images might override your instructions or cause Claude to make mistakes."

> "classifiers will automatically scan what the tools return, such as screenshots, to flag potential prompt injections ... This extra protection won't be ideal for every use case (for example, use cases without a human in the loop), so if you'd like to opt out and turn it off, contact support."

> "Inform end users of relevant risks and obtain their consent prior to enabling computer use in your own products."

The Agent SDK hosting docs add: route egress through a proxy "that enforces domain allowlists, injects credentials, and logs requests", and isolate with Docker, gVisor or Firecracker. Source: https://code.claude.com/docs/en/agent-sdk/hosting

**Overlord-specific threat model [I]:**
- **Chat is a prompt-injection channel twice over.** Viewers can type instructions ("Overlord, go to evil.site and paste...") and can also plant content on pages the agent visits. Rules:
  - Never pass raw chat into the slow lane. Give it only Director-curated "suggestions" as data.
  - Run the browser in a **throwaway VM** with no personal accounts, no password manager, no payment methods, and a domain allowlist.
  - The stream shows the VM screen only, never the host desktop.
  - Block form submits, downloads, and logins via `PreToolUse` hooks.
  - Have a human "kill switch" hotkey.
- The injection classifier's "check whether the instruction really came from you" steering can stall an unattended agent. Expect it, and handle it in the Director (time out and move on). Don't opt out early.
- Leaking secrets on screen is a stream-specific risk. Use an OBS scene with a blur/blackout layer when the agent is near address bars or settings.

---

## 6. How competing engines reach low latency (brief)

| Engine | Approach | Latency (claims) | Notes |
|---|---|---|---|
| **OpenAI Realtime** (gpt-realtime-2 / 2.1) | Single speech-to-speech model, WebRTC/WebSocket/SIP | [S] ~200 ms model response (anecdotal), ~800 ms once RAG is added | [S] ~$32 / $64 per MTok of audio in/out, cached audio $0.40 (https://aireiter.com/blog/openai-realtime-api-pricing, https://www.forasoft.com/blog/article/openai-realtime-api-pricing) |
| **Gemini Live API** | Stateful WebSocket, native-audio model, 16 kHz in / 24 kHz out | [S] "<500 ms" claims, no official figure | [V-ish, Firebase docs] connection ~10 min, audio-only session 15 min, 128k context, so it needs session resumption plus context compression for multi-hour use (https://firebase.google.com/docs/ai-logic/live-api/limits-and-specs) |
| **xAI Grok Voice Agent** | Realtime voice API, OpenAI-Realtime-compatible | [S] ~0.78 s average time-to-first-audio (xAI internal) | [S] $0.05–0.08/min (https://www.eesel.ai/blog/grok-voice-think-fast-2-pricing) |
| Wrapped voice platforms | STT → LLM → TTS pipelines | [S] 680–850 ms median voice-to-voice (https://telnyx.com/resources/voice-ai-agents-compared-latency) | |

**[I] Lesson for Overlord:**
- The others hit sub-second latency with **speech-native models plus persistent sockets**. Overlord's 2–4 s budget is generous by comparison, so a cascaded STT → Haiku 5.5 → streaming TTS pipeline fits.
- A speech-to-speech engine only becomes worth it if Overlord takes live *voice* input (co-host calls). Even then, the speech-native session limits (Gemini's 15 min) push you back to the same "external memory + session recycle" pattern as §1.2.
- Claude has no native speech-to-speech API, so the voice layer is always a separate vendor.

---

## 7. Go / no-go risk register

| # | Risk | Severity | Status / mitigation |
|---|---|---|---|
| R1 | **Running production on Pro/Max subscription** breaches the Consumer Terms (automated access, commercial use). Enforcement "without prior notice" could mean an account ban mid-stream. | **High** | **No-go.** Use a Console API key for anything public. Keep the subscription for dev only. [V] |
| R2 | **Platform pivot:** hosting Claude Code for others requires per-end-user credentials and bars intermediating usage. Resale needs Anthropic approval. | **High (strategic)** | Build on the Messages API + Agent SDK under the Commercial Terms. Get sales sign-off before per-usage pricing. Follow branding rules. [V+I] |
| R3 | **Fast-lane latency** if built on Claude Code or Opus (always-on thinking). | Med | Messages API + Haiku 5.5 with thinking disabled. Benchmark p50/p95 TTFT before committing. [V+I] |
| R4 | **API spend caps** (Build $1k/mo) stop the stream mid-month. | Med | Request the Scale tier, set your own alerts, track `total_cost_usd`. [V] |
| R5 | **Session decay** over hours (compaction drift, RAM growth, cache misses). | Med | External memory, scheduled session recycling at segment breaks, watchdog. [V+I] |
| R6 | **Prompt injection via chat and web pages** in computer use. | High | VM sandbox, domain allowlist, no creds, hooks, Director filter, kill switch. [V+I] |
| R7 | **Content policy:** villain bit drifting into real-world targets, extremism or violent glorification. Refusals mid-bit. | Med | Fictional framing, red lines, chat moderation lane, in-character refusal handling, on-screen AI disclosure. [V+I] |
| R8 | **Cost blow-up** from cache misses or bloated context. | Med | Measure `cache_read_input_tokens`. Batch chat. Haiku ≤100k prompt. Never flip effort or thinking mid-stream. [V+I] |
| R9 | **Research-preview dependencies** (Channels, some betas) changing. | Low–Med | Don't make channels load-bearing; the Director feeds the SDK `streamInput()` directly. [V] |
| R10 | AUP update effective **2026-11-12** (new deceptive-activity and model-abuse sections). | Low | Re-read before launch. Disclose AI, and no fake engagement. [V/S] |

---

## 8. Sources

**Official (Anthropic)**
- Claude Code headless / Agent SDK CLI: https://code.claude.com/docs/en/headless
- Agent SDK overview (auth note, branding, license): https://code.claude.com/docs/en/agent-sdk/overview
- Agent SDK quickstart (auth): https://code.claude.com/docs/en/agent-sdk/quickstart
- Streaming vs. single mode: https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode
- Sessions: https://code.claude.com/docs/en/agent-sdk/sessions
- Hosting: https://code.claude.com/docs/en/agent-sdk/hosting
- Prompt caching in Claude Code: https://code.claude.com/docs/en/prompt-caching
- Costs / long sessions / thinking note: https://code.claude.com/docs/en/costs
- Channels: https://code.claude.com/docs/en/channels
- Authentication / setup-token: https://code.claude.com/docs/en/authentication
- Legal & compliance: https://code.claude.com/docs/en/legal-and-compliance
- Docs index: https://code.claude.com/docs/llms.txt
- Models overview: https://platform.claude.com/docs/en/about-claude/models/overview
- Haiku 5.5: https://platform.claude.com/docs/en/models/haiku-5-5/overview
- Pricing: https://platform.claude.com/docs/en/about-claude/pricing
- Rate limits and spend caps: https://platform.claude.com/docs/en/api/rate-limits
- Thinking (disable / between_tools): https://platform.claude.com/docs/en/build-with-claude/thinking
- Reduce latency: https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/reduce-latency
- Computer use tool: https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool
- Consumer Terms: https://www.anthropic.com/legal/consumer-terms
- Commercial Terms: https://www.anthropic.com/legal/commercial-terms
- Usage Policy: https://www.anthropic.com/legal/aup
- Pro/Max with Claude Code (limits): https://support.claude.com/en/articles/11145838-use-claude-code-with-your-pro-or-max-plan
- Local: `claude --help`, Claude Code 2.1.302. Bundled `claude-api` skill reference (model, thinking and fast-mode notes).

**Secondary**
- Artificial Analysis, Haiku 5.5: https://artificialanalysis.ai/models/claude-haiku-5-5-low and the comparison page above
- llm-stats, Haiku 5.5: https://llm-stats.com/models/claude-haiku-5-5
- Usage-limit guides: https://krater.ai/blog/claude-usage-limits, https://wokey.ai/claude-usage-limits, https://portkey.ai/blog/claude-code-limits/
- OAuth enforcement coverage: https://paddo.dev/blog/anthropic-walled-garden-crackdown/, https://natural20.com/coverage/anthropic-banned-openclaw-oauth-claude-code-third-party, https://news.creeta.com/en/claude-code-oauth-block-2026-third-party-tools/
- AUP update coverage: https://www.unite.ai/anthropic-updates-usage-policy-adding-model-abuse-and-deception-rules/
- RTX 4090 local LLMs: https://openclawdc.com/blog/best-local-llm-rtx-4090/, https://modelfit.io/gpu/rtx-4090/, https://atomic.chat/blog/guides/best-local-llms-for-rtx-4090, https://computingforgeeks.com/best-gpu-for-llm/
- Voice engines: https://telnyx.com/resources/voice-ai-agents-compared-latency, https://aireiter.com/blog/openai-realtime-api-pricing, https://www.forasoft.com/blog/article/openai-realtime-api-pricing, https://firebase.google.com/docs/ai-logic/live-api/limits-and-specs, https://www.eesel.ai/blog/grok-voice-think-fast-2-pricing

---

## 9. Open questions and next experiments [I]

1. **TTFT bench** with the real persona prompt: Haiku 5.5 (thinking disabled, low and medium effort) vs. Sonnet 5.5 (`between_tools`, low effort), 200 calls each, p50/p95, warm vs. cold cache.
2. **Agent SDK soak test:** a 6-hour streaming-input session with the memory MCP and the browser in a VM. Measure RSS growth, compaction events, cache hit rate, and `total_cost_usd`.
3. **Ask Anthropic sales:** (a) is a monetized single-operator AI streamer on an API key fine under the Commercial Terms (expected yes)? (b) What is permitted for platform resale of character "hours"?
4. **Persona red-team:** about 200 adversarial chat prompts. Measure refusal rate and in-character deflection quality per model.
5. Check Twitch and YouTube AI-disclosure and violence policies (separate research doc).
