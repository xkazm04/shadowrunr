# 06 — Moderation and monetisation: platform rules, moderation tools, term rules

Research date: **2026-10-18**. Scope:
- Twitch: Community Guidelines and their 2023–2026 changes, AutoMod, chat safety modes, the
  Helix moderation API, and rules on bots and branded content.
- Third-party moderation bots and AI moderation services.
- YouTube: advertiser-friendly rules, Shorts, inauthentic and reused content.
- TikTok: guidelines, For You feed (FYF) eligibility, LIVE.
- The term rules used by the output gate and the clip factory's word cutter.

Machine-readable output: `policy/platform-rules.json` (schema in `docs/spec/contract.md`).

**How claims are marked.**
- `[m12]`: a source id from the table at the end.
- **[R]**: my recommendation, not a platform statement.
- **[S]**: a secondary or snippet-only source.

Twitch's newer `safety.twitch.tv/s/...` and `help.twitch.tv/s/...` pages render only with
JavaScript, and legal.twitch.com would not render either. The older
`/articles/en_US/Knowledge/<Article>` URLs return full text, so they are the URLs cited.

---

## TL;DR

1. **Twitch's risk to Overlord is moderation, not AI.**
   - Twitch has no AI-disclosure rule [m10].
   - The streamer is responsible for "all content in your streams, including policy
     violations by third parties" [m01].
   - A TTS-read slur has already cost a human streamer a strike [m56].
   - Both AI-stream bans so far were for hateful conduct [m58][m59].
2. **AutoMod is a first net, not the gate.** An ACL 2025 audit found recall of 0.22 at the
   strictest setting, up to 94% of hateful messages passing on some datasets, and up to 98.5%
   of benign sensitive-term messages flagged [m13]. It also only acts on chat, never on what
   Malakor says.
3. **YouTube relaxed the opening-seconds profanity rule in July 2025** [m35]. Strong
   profanity in titles and thumbnails still kills ads [m34]. The bigger risk for a clip
   channel is the **inauthentic content** policy: "characters put in the same situation over
   and over again with the same outcome" is listed as ineligible [m41].
4. **TikTok LIVE holds the host responsible for third-party tools, including on-screen
   comment displays** [m43]. It bans degrading someone with profanity in LIVE [m46]. TikTok
   is a clip venue first.
5. **Recommendation: hybrid.**
   - **Build** what only we can see: the path-independent output gate, the chat-picker
     pre-filter, and the term rules.
   - **Integrate** Twitch's native tools through Helix: AutoMod, Blocked Terms, Shield Mode,
     verified chat and chat delay.
   - **Don't adopt** Nightbot, StreamElements or Fossabot as safety layers.

   Details and the anti-bait rules for the chat picker are in §6.

---

## 1. Twitch

### 1.1 Community Guidelines (current structure)

The guidelines are grouped into Safety, Civility and Respect, Illegal Activity, Sensitive
Content and Authenticity [m01]. The parts that bind Overlord:

| Area | Rule | Source |
|---|---|---|
| Hateful conduct | "Twitch does not permit behavior that is motivated by hatred, prejudice or intolerance." Protected characteristics: age, race, ethnicity, colour, caste, national origin, immigration status, religion, sex, gender, gender identity, sexual orientation, disability, serious medical condition, veteran status. | [m01][m02] |
| Harassment | Behaviour that is "mean, cruel, disrespectful, attacking, shaming or insulting", including vague threats | [m03] |
| Violence and threats | "considered a zero-tolerance violation" | [m01] |
| Self-harm | No content that "glorifies, promotes, or encourages self-harm" | [m01] |
| Sexual content and nudity | Bans "depictions of real or fictional nudity, regardless of the medium used to create it", digital or not. AI is not named. | [m01] |
| Private information | Unauthorised sharing of private information is prohibited | [m01] |
| Authenticity | No impersonation, no spam ("large amounts of repetitive, unwanted messages"), no viewership tampering, no suspension evasion | [m01] |
| Responsibility | "Streamers are expected to use channel moderators and the tools we provide"; "you are responsible for all content in your streams, including policy violations by third parties" | [m01] |

### 1.2 Changes, 2023–2026

| Date | Change | Source |
|---|---|---|
| 2023-06-20 | Six Content Classification Labels: Mature-Rated Games; Sexual Themes; Drugs, Intoxication or Excessive Tobacco Use; Violent and Graphic Depictions; Significant Profanity or Vulgarity; Gambling. No suspension for mislabelling. | [m04] |
| 2023-07-01 | Branded Content Guidelines enforced | [m22] |
| 2023-12-13 → 12-15 | "Artistic nudity" loosening rolled back after two days ("we went too far") | [m05] |
| early 2024 | Ban on streams focused on clothed intimate body parts and on implied nudity. Sexual-Themes streams removed from homepage recommendations. | [m06] |
| 2024-09-20 | Expiring strikes announced: "Low-severity strikes will no longer put streamers' livelihoods at risk" | [m08] |
| 2024-11-15 | "Zionist" treated as a proxy for Jews or Israelis when used to promote harm. Political discussion is still allowed. | [m01][m07 S] |
| 2025-02-19 | Strike expiry launched. Low-severity strikes reportedly expire after about 90 days, and the highest-harm ones never expire. | [S, not confirmed on an official page] |
| 2025-08 | Viewbot crackdown; reported platform viewership fell about 24% week over week | [m61 S] |
| 2026-02-24 | Separate "streaming suspensions" and "chatting suspensions" under a targeted enforcement system; temporary suspensions run from 24 h to 30 days | [m09] |
| 2026-09-20 | CEO: no AI-disclosure rule for now; Twitch would act "If it became prevalent for streamers to be substituting themselves" | [m10] |

I found no 2025 or 2026 change to the hateful conduct or harassment text itself, and no
Twitch rule on AI-generated streamers or unattended streams.

### 1.3 AutoMod

The table below is Twitch's own level mapping [m11]. "Some", "more" and "most" are Twitch's
words for how aggressively each category is filtered.

| Level | Discrimination & Slurs | Sexual Content | Harassment | Profanity | Smart Detection |
|---|---|---|---|---|---|
| 0 | — (Blocked Terms still apply) | — | — | — | — |
| 1 | some | — | — | — | yes |
| 2 | some | some | more | — | yes |
| 3 | more | more | more | — | yes |
| 4 | more | more | most | more | yes |

How it works:
- **Held messages.** Mods choose Allow or Deny. "AutoMod does not timeout, ban, or mute
  users." An approved message appears as the next message in chat [m11].
- **The API's granular levels.** The API exposes eight categories, each set 0–4 [m12]:
  disability, aggression, sexuality_sex_or_gender, misogyny, bullying, swearing,
  race_ethnicity_or_religion, sex_based_terms. Setting `overall_level` overwrites them all.
- **Twitch's recommended baseline** (2026-10-01) [m14]:
  - AutoMod at level 2.
  - Email and phone verification on.
  - Follower-only mode for at least 10 minutes.
  - Shield Mode set up for when it is needed.
- **Accuracy.** About 300k test messages (December 2024–January 2025) gave recall of 0.22 at
  the strictest setting. AutoMod depends heavily on slur matching and has no awareness of
  conversation context [m13].

**[R] Overlord setting:**
- race_ethnicity_or_religion, disability, sexuality_sex_or_gender and misogyny at 3.
- sex_based_terms, aggression and bullying at 2.
- swearing at 0, because the audience is adult and the channel carries the "Significant
  Profanity or Vulgarity" label.

Messages held while no human mod is watching are effectively dropped, which is acceptable.

### 1.4 Chat safety modes

| Tool | What it does | Source |
|---|---|---|
| Blocked Terms | A wildcard (`hate*`) blocks variants. Multi-word terms match only when all words appear. Capacity is "in the tens of thousands". Lists are public (visible to mods) or private (owner only). Through the API: 2–500 characters, `*` only at the start or end, public list only. | [m11][m12] |
| Permitted Terms | No wildcards | [m11] |
| Shield Mode | A one-click preset: follower- or sub-only chat, verification, AutoMod levels, a "No First-Time Chatters" mode, and a term-based mass ban that resets each session. `/shield`, `/shieldoff`. Turning it off restores the previous settings. | [m15] |
| Chat delay (non-mod) | 2, 4 or 6 s | [m17] |
| Slow mode | 3–120 s, default 30 | [m17] |
| Follower-only | 0 min to 3 months (0–129,600 min) | [m16][m17] |
| Emote-only | Messages must be 100% emotes | [m16] |
| Unique chat | At least 9 non-symbol characters | [m16] |
| Phone/email-verified chat | Can be required of all accounts, first-time chatters, accounts younger than 1 h–6 months, or followers of less than 10 min–3 months. VIPs, subs and mods can be exempted. Up to five accounts per phone number. Off by default. | [m18] |
| Suspicious-user detection | Flags "likely" and "possible" ban evaders and hides messages from likely evaders. On by default and never bans automatically. | [m62 S] |

### 1.5 Helix API and EventSub (for the daemon's moderation adapter)

| Need | Endpoint | Scope | Source |
|---|---|---|---|
| Read or set AutoMod levels | `GET`/`PUT /helix/moderation/automod/settings` | `moderator:read:automod_settings` / `moderator:manage:automod_settings` | [m17] |
| Approve or deny a held message | `POST /helix/moderation/automod/message` (`ALLOW`/`DENY`) | `moderator:manage:automod` | [m17] |
| Pre-check a message against AutoMod | `POST /helix/moderation/enforcements/status`. Up to 100 messages per call; 5/min for normal channels, 10/min for Affiliates, 30/min for Partners. | `moderation:read` | [m17][m63] |
| Blocked terms | `GET`/`POST`/`DELETE /helix/moderation/blocked_terms` | `moderator:manage:blocked_terms` | [m17] |
| Shield Mode | `GET`/`PUT /helix/moderation/shield_mode` (`is_active`) | `moderator:manage:shield_mode` | [m17] |
| Chat modes and delay | `PATCH /helix/chat/settings` | `moderator:manage:chat_settings` | [m17] |
| Ban or timeout | `POST /helix/moderation/bans` (timeouts 1 s to 2 weeks; reason at most 500 characters) | `moderator:manage:banned_users` | [m17] |
| Delete a message | `DELETE /helix/moderation/chat` (message under 6 h old; not from a mod or the broadcaster) | `moderator:manage:chat_messages` | [m17] |
| Warn a user | `POST /helix/moderation/warnings` | `moderator:manage:warnings` | [m17] |
| Events | `automod.message.hold` v2 (gives the reason and the matching blocked term), `channel.chat.message`, `channel.suspicious_user.message`, `channel.moderate` v2 | per subscription | [m19] |

### 1.6 Bots and branded content

- **Chat bot identity:**
  - Send through the Send Chat Message API with an app access token. The bot account grants
    `user:bot`; the channel grants `channel:bot` or makes the bot a mod.
  - This shows the **Chat Bot badge** (introduced 2025-08-28; a user token does not qualify)
    [m21].
  - Rate limits: 20 messages per 30 s for a normal account, 100 per 30 s for a mod, VIP or
    broadcaster. Over the limit, Twitch ignores the bot for an hour. Verified-bot reviews
    are paused.
  - EventSub plus the API is the preferred method; IRC is legacy [m20].
- **Terms of Service:** the ToS text on automated access could not be retrieved
  (legal.twitch.com did not render). A secondary quote bans access by "robot, spider,
  scraper, crawler or other automated means" [S]. Treat the bot account as a declared Chat
  Bot, never as a human-looking account.
- **Branded content:**
  - The paid-promotion disclosure is required when branding is "a central and intentional
    element".
  - Banned promotions: risky gambling (slots, roulette sites), weapons, adult content,
    tobacco, political content and cannabis [m22].
  - This matches the design digest's red line: no real crypto or stock promotion.

---

## 2. Third-party moderation bots and AI moderation services

| Tool | What it automates | Cost | Programmable by another app? | Source |
|---|---|---|---|---|
| Nightbot | Blacklist; caps, emote, link (with allowlist), symbol and repetition filters; timers; commands | Not shown | Yes: REST API with OAuth2. `POST /1/channel/send` (400 characters, 1 request per 5 s) and `PUT /1/spam_protection/:type` | [m23][m24] |
| StreamElements | Banned words (regex), caps, emotes, language, links, one-man spam, paragraph, repetition, symbols, zalgo. Timeout, ban, delete or warn. Presets from minimum to maximum. | Free | An API exists (JWT, snippet only) | [m25] |
| Fossabot | 15 filters; blocked terms with regex and "Lookalikes" (catches spellings like "fr33"); malware-link filter | "Fossabot is free" | No API found | [m26] |
| Sery_Bot | Hate-raid, bot and spam protection. `!hateraidon` toggles Shield Mode; follow-bot cleanup | "forever free" | No API found. Reached 55,000+ integrations within 2 months during the 2021 hate raids. | [m27][m57] |
| Moobot | Filters, link and clip allowlists, blocked words; can auto-deny messages AutoMod held | Freemium (price not found) | Not found | [m28] |
| Streamlabs Cloudbot | Caps, symbol, link (`!permit`), blacklist and paragraph protection | Not found | Not found | [m29 S] |
| OpenAI Moderation (`omni-moderation-latest`) | Classifies text and images into 13 categories (harassment, hate, illicit, self-harm, sexual incl. minors, violence, and sub-categories) | "free to use" | Yes (HTTP API) | [m30] |
| Perspective API | Toxicity scores | Free, but **shutting down**: active "until December 31, 2026", new usage requests closed in February 2026 | Yes, until the shutdown | [m31] |
| Hive | Text moderation | $0.50 per 1,000 requests; developer tier 100 requests per day | Yes | [m32] |
| Claude (Anthropic guide) | An LLM classifier: messages and categories in tags, JSON output, optional 0–3 risk levels, batching when real time is not needed. The guide recommends Haiku for cost. | Per token | Yes; already in our stack | [m33] |

**What the bots cannot do.** All of these act on *chat as other viewers see it*: they delete
or time out messages. None can moderate:
- **what the model reads**: which messages reach Malakor's prompt;
- **what the model says**: Malakor's spoken lines, foil lines and fallback-path output;
- **what the clip factory publishes**: captions, titles and bleeps.

Nothing, Forever's ban came from a fallback model path that skipped the filter [m58]. That
failure lives entirely inside our own pipeline.

---

## 3. YouTube

### 3.1 Advertiser-friendly guidelines: profanity

| Ad state | Profanity examples | Source |
|---|---|---|
| Green (full ads) | Obscured profanity; "hell" or "damn" in the title, thumbnail or video; moderate words ("bitch", "shit") used often in the video; most profanity in music or stand-up | [m34] |
| Yellow (limited) | Moderate profanity ("shit") in the title or thumbnail; focal profanity throughout a video; profanity in titles of music or stand-up content | [m34] |
| Red (no ads) | "Stronger profanity (like f*ck) used in thumbnails or titles"; extreme profanity or slurs anywhere | [m34] |

**Opening seconds, over time:**
- **Nov 2022:** profanity in the opening 15–20 s was restricted [m37].
- **Mar 2023:** the f-word in the first 7 s moved from no ads to limited ads; profanity at
  8–15 s became eligible [m35][m37].
- **Jul 2025:** "Use of stronger profanity (e.g. f*ck) in the first 7 seconds of content can
  now be eligible to earn ad revenue" [m35]. High-frequency use (e.g. "a compilation video of
  the best swearing") is still restricted, and the title and thumbnail rules are unchanged
  [m36].

### 3.2 Other advertiser-friendly areas, briefly

- **Violence:** game violence after the first 7 s is fine; gore and glorification earn no
  ads. Since Sept 2026, graphic gaming violence at 8–15 s is eligible [m34][m35].
- **Adult content:** sexual themes in a thumbnail or title limit ads; sexual acts, even
  implied, earn none [m34].
- **Hateful content:** satire is possible, but "stating comedic intent alone isn't enough"
  [m34].

### 3.3 Shorts

- **Eligibility:** Shorts earn only with the Shorts Monetization Module accepted. Creators
  keep 45% of allocated revenue [m38].
- **Views that don't count:** non-original Shorts (unedited clips, re-uploads, compilations
  with no original content) and Shorts that are not advertiser-friendly [m38].
- **3-minute Shorts:** monetise if uploaded after 2024-10-15. Shorts over one minute with
  claimed content are blocked [m38][m39].
- **From 2027-02-01:** eligibility needs 10M qualified Shorts views in 90 days [m38].
- **Oct 2026:** less reach for re-uploads with minimal edits, automated voiceovers or
  template changes [m40].

### 3.4 Inauthentic and reused content

On 2025-07-15, "repetitious content" was renamed **inauthentic content** [m41]. Ineligible
examples:
- "templated storylines", and "characters … put in the same situation over and over again
  with the same outcome";
- channels whose content "feels interchangeable";
- "AI-generated content made with generic or unoriginal templates giving the impression of
  mass production".

**Reused content** is also ineligible: "Clips of moments … edited together with little or no
narrative", compilations from other sites, and readings of material you did not create. The
rule applies "even if you have permission" and can demonetise the whole channel [m41].

**Synthetic-content disclosure** is required only for *realistic* content: a real person
appearing to say or do something, altered real events, realistic AI music. Animation and
unrealistic content are exempt, and disclosure alone does not limit reach or monetisation
[m42]. Malakor is exempt, but labelling him "AI character" is still cheap insurance
(`01-market-landscape.md` §5).

**Livestreams.** Advertiser-friendly guidelines apply to live streams. Supers pay creators
70% [m41 and the YouTube live monetisation pages, S]. No inauthentic-content rule is specific
to livestreams.

---

## 4. TikTok

The current Community Guidelines are **effective 2026-09-24** [m43]. They replace the
2025-09-13 version, which dissolved the standalone list of FYF-ineligible content into each
section and made LIVE creators responsible for third-party tools [m44].

| Area | Rule | Source |
|---|---|---|
| Hate | No hateful slurs tied to a protected attribute, dehumanisation or misgendering. Indirectly demeaning content may be FYF-ineligible. | [m43] |
| Sexual | Nudity, sexual activity and services not allowed. Suggestive adult content is 18+ and FYF-ineligible. Explicit sexual language is restricted in some regions. "Sexual language used in non-sexual contexts, such as humor or stand-up comedy" is allowed. | [m43] |
| Violence and shock | Gore and moments of death not allowed. Excessive blood and graphic fiction are 18+ and FYF-ineligible. Shocking audio or visual effects ("jump scare") are FYF-ineligible. | [m43] |
| Self-harm | Detailed methods, hoaxes and plans not allowed; non-graphic fiction is 18+ and FYF-ineligible | [m43] |
| AI content | Label realistic AI depictions (AIGC label, caption, watermark or sticker). Not required for anime styles or "generic text-to-speech (TTS) narration, when the TTS isn't a recognizable voice". | [m43] |
| Unoriginal content | Reused material "without anything new" and minimally edited clips are FYF-ineligible | [m43] |
| LIVE: age | 18+ to go LIVE, plus a follower minimum per market. 18+ to send or receive gifts (19 in South Korea). | [m45] |
| LIVE: third-party tools | Creators are responsible "even when it involves third-party tools like voice-to-text software, real-time translation, or on-screen comment displays"; repeated issues may restrict LIVE | [m43] |
| LIVE: FYF-ineligible | Pressuring for gifts; unoriginal content without new edits; "repeated or prolonged actions that lack clear objectives"; blank or low-quality screens; off-platform selling | [m43] |
| LIVE: conduct | "Degrading someone with profanity or obscene language"; "Do not attack others with vulgar speech". "Streaming previously recorded content in TikTok LIVE is not recommended." | [m46] |
| Profanity elsewhere | No general profanity rule for videos. Comments with "Profanity that's non-neutral or targets others" may be ranked lower. | [m43] |
| LIVE monetisation | Gifts, Subscriptions and Creator Rewards require the Creator Code of Conduct and the LIVE Monetization Guidelines. Since 2024-05, graphic, inauthentic or low-quality LIVE content cannot earn. | [m43][m47 S] |
| Music | Business accounts must use the Commercial Music Library | [m48 S] |
| Shop LIVE | Reported ban on AI voices and on virtual characters covering more than half the screen in US Shop LIVEs. Secondary source only. | [m49 S] |

---

## 5. Term rules for the output gate and the clip factory

### 5.1 Categories and actions

`policy/platform-rules.json` → `term_rules` (23 rules):

| Category | Rules | Action | Basis |
|---|---|---|---|
| `slur` | `slur-operator-list`, `slur-operator-list-alt` | block | Twitch hateful conduct; YouTube no-ads; TikTok hate. **No terms embedded** (see 5.2). |
| `profanity-advertiser` | `profanity-strong-clip` (YouTube, TikTok) | bleep | YouTube no-ads for strong profanity in titles and thumbnails, and density limits [m34][m36] |
| | `profanity-strong-live` (Twitch) | warn | Allowed on Twitch with the Significant Profanity label [m04] |
| | `profanity-moderate` | warn | Green in video, yellow in titles [m34]. Logged for density scoring. |
| | `profanity-directed-tiktok` | bleep | TikTok LIVE: degrading someone with profanity [m46] |
| | `profanity-directed-warn` (Twitch, YouTube) | warn | Harassment risk [m03] |
| `sexual` | `sexual-explicit`, `sexual-minors` | block | Red line 2; Twitch, YouTube and TikTok sexual rules |
| | `sexual-suggestive` | warn | YouTube title/thumbnail limits; TikTok FYF age restriction |
| `violence` | `violence-direct-threat` | block | Twitch zero tolerance [m01] |
| | `violence-graphic`, `violence-mass-casualty` | warn | Context matters (theatrical "eviscerate"; news) |
| `harm-instructions` | `harm-instructions` | block | Red line 6 |
| `self-harm` | `self-harm-incitement` | block | Twitch self-harm rule [m01] |
| | `self-harm-disclosure` | warn | Input-side signal: route to a sincere response, never a roast |
| `pii` | `pii-email`, `pii-phone` | block | Twitch private information [m01]; red line 4 |
| | `pii-street-address` | warn | |
| `brigade` | `brigade-call` | block | Design guardrail 3 (zero crowd action); hate-raid history [m57] |
| `injection` | `injection-override`, `injection-authority-prefix`, `injection-obfuscation` | warn | Input tagging for the chat picker (OWASP LLM01) |

Action semantics [R]:
- **block** at input: drop the message from selection and emit `Moderation{stage: input}`.
  At output: do not speak or display the line; replace it with the in-character redaction
  gag and emit `Moderation{stage: output}`.
- **bleep:** live speech replaces the matched span with a redaction token or tone. The clip
  factory bleeps the audio and masks the caption (`f***`). Titles and thumbnails must contain
  no bleep-category term at all.
- **warn:** allowed and logged. Used for:
  - profanity density, so a clip that is "throughout the majority" profanity is dropped or
    bleeped;
  - context flags (self-harm disclosure, injection attempt).
- **One action per rule.** When rules overlap, the consumer applies the strictest action
  among rules that list the current platform. A Twitch live line and the clip cut from it
  get different treatment from the same file.

### 5.2 Slurs: an operator-installed list, never embedded

The two `slur` rules carry a **placeholder pattern** that compiles but never matches:
`\b\B__OPERATOR_INSTALLED_LIST__<list>__<categories>__`. A word boundary and a non-boundary at
the same position cannot both hold, so the rule is inert until the operator installs a list.
The loader must recognise the `\b\B__OPERATOR_INSTALLED_LIST__` prefix and replace the rule
with patterns built from the installed file.

| List | Licence | Maintained? | Slur categories? | Use |
|---|---|---|---|---|
| **awdev1/better-profane-words** (https://github.com/awdev1/better-profane-words) | GPL-3.0 (checked through the GitHub API) | Yes, last push 2026-05-13 | `slur_racial` (412), `slur_gender` (116), intensity 1–5 | **Primary** (`slur-operator-list`). The operator installs it locally, so Overlord does not redistribute GPL data. |
| Surge AI profanity list, copy at NVIDIA/garak `garak/data/profanity_en.csv` (https://github.com/NVIDIA/garak/blob/main/garak/data/profanity_en.csv) | MIT according to a 2022 snapshot of the deleted upstream repo [S]. The garak file carries no licence notice; the garak repo is Apache-2.0. | Upstream deleted; the copy was last touched 2024-09-17 | `racial / ethnic slurs`, `sexual orientation / gender`; severity Mild, Strong or Severe from 5 raters | **Alternative** (`slur-operator-list-alt`) where GPL is unwanted. Confirm the licence before redistributing. |
| LDNOOBW (https://github.com/LDNOOBW/List-of-Dirty-Naughty-Obscene-and-Otherwise-Bad-Words) | CC-BY-4.0 | Last push 2024-08-05 | None (flat list, 28 languages) | Multilingual fallback only; it mixes profanity and slurs |
| obscenity (npm, jo3-l) | MIT | Active (2026-08) | None | A matcher design to copy: transformers for leetspeak and Unicode variants |

Hatebase was retired in 2022. Weaponized Word is commercial. Neither is an option.

**Installation [R].** The operator puts the list file at
`policy/installed/<list>.{txt,csv}`. Only rows in the slur categories are used. The loader
builds case-insensitive, word-bounded patterns with a normalisation pass first: NFKC
normalisation, leetspeak folding, and collapsing repeated letters and separators. That pass
handles the emote, cipher and lookalike evasion documented in `05` §4. Without an installed
list, the output gate must still refuse to start live. This fits pre-flight check PF-08.

### 5.3 Regex conventions

- The patterns target the Rust `regex` crate. They use no lookaround and no backreferences,
  and use `(?i)` and `\b` throughout. They were validated in Python with 70+ positive and
  negative samples (e.g. "Scunthorpe", "shiitake", "assassin", "cum laude", "2026-10-18",
  "HEX-06" and "Case 26-CV-0412" must not match).
- Embedded patterns cover only the advertiser-sensitive and safety categories. Slurs are
  never embedded.
- "hell" and "damn" are deliberately absent: they are green everywhere [m34].
- Known limits: regex cannot see context (theatrical "I will annex you" passes, as intended)
  and cannot catch obfuscated slurs. That is the job of the normalisation pass and the LLM
  stage in §6.

---

## 6. Recommendation: automated moderation and anti-bait rules

### 6.1 Build, integrate or hybrid? **Hybrid.**

| Layer | Decision | Why |
|---|---|---|
| Output gate (Malakor, foils, fallback paths, clip captions and titles) | **Build** (WP2 and the clip factory) | No third-party tool sees model output. Path independence is the documented failure mode [m58]. |
| Chat-picker pre-filter (what the model reads) | **Build** (WP2) | Bots act on chat display, not on model input. Anti-bait tactics target the model, not other viewers. |
| Term rules and the slur list | **Build** the file and loader; **install** an open list | Platform-specific actions differ (bleep for YouTube and TikTok, warn on Twitch). |
| Ambiguous-case classifier | **Build** on Claude Haiku (already in the stack), batched [m33] | Avoids a second vendor and sending chat elsewhere. Perspective is shutting down [m31]. OpenAI Moderation is free [m30] but adds a vendor, so it is an optional benchmark only. |
| Chat as viewers see it (held messages, blocked terms, slow/follower/verified modes, Shield Mode, delay) | **Integrate** Twitch native tools through Helix (§1.5) | Free and platform-native. They act before other viewers see a message, and they are what Twitch expects streamers to use [m01][m14]. |
| Hate-raid response | **Integrate**: Shield Mode through the API, triggered by our own raid detector. Sery_Bot optional. | Raids finish in seconds [m57], so the trigger must be automatic. |
| Nightbot, StreamElements, Fossabot, Moobot, Cloudbot | **Do not adopt** as a safety layer | They duplicate native filters (caps, links, symbols); Fossabot and Sery have no API, and Nightbot's is limited to 1 send per 5 s. They can be added later for viewer commands and timers. |
| Human mods | **Keep** at least one per live show | Twitch expects mods [m01]; they become important past about 6 regular viewers (`05` [c04]). Held AutoMod messages need a human. |

### 6.2 Anti-bait rules the chat picker must enforce

Thresholds marked [A] are assumptions to tune in the sim.

1. **AB-01 Injection never reaches the prompt verbatim.** A message matching `injection-*`
   is tagged `injection_attempt` with the viewer's name. The core thread receives only the
   tag, so Malakor can mock the attempt without seeing its payload. This is red line 9.
2. **AB-02 No decoding, spelling or assembling.** Malakor never decodes, deciphers, reverses
   or spells out a chat-supplied string. He never builds words from letters across
   messages, and never reads "the first letters" of anything (`obfuscated-slur`,
   `payload-split`).
3. **AB-03 No verbatim repetition.** Malakor never repeats a chat-supplied phrase of more than
   4 words [A]. "Repeat after me", "say exactly" and "say X or you're a coward" are refused
   in character (`repeat-after-me`, `say-x-dare`).
4. **AB-04 Authority comes from metadata only.** `is_mod` and broadcaster status come from the
   platform viewer record. Text prefixes (`[MOD]`, `SYSTEM:`, "the Lord says") mark the
   sender as a baiter. The Lord speaks only through the dispatch, never through chat.
5. **AB-05 No real people on request.** A message naming a real person is not picked for
   roasting or opinion. The only exception is a public entity in the active, owner-approved
   SEG-02A case, and then only with grade-correct "alleged" language.
6. **AB-06 Loaded questions get deflection, not engagement.** Questions about atrocities,
   protected groups or conspiracy theories are picked only for an in-character deflection.
   They are never answered factually by the fast lane (`loaded-history-question`).
7. **AB-07 Waves collapse to the cleanest representative.** Near-duplicates collapse into one
   message with a count. The representative is the variant that passes every term rule, so
   a payload never wins because it was the first variant (`copypasta-payload`). Malakor
   answers a wave as one crowd voice.
8. **AB-08 Caps per viewer and per rank.** At most one pick per viewer per 60 s at 30+ CCV
   [A]. Rank weight is capped so a whale or gift-bomber cannot monopolise attention or votes
   (`rank-and-vote-manipulation`).
9. **AB-09 Raid handling.** For the first 60 s after a raid [A], greet the raid as a group
   and pick no individual raider. First-time chatters and accounts younger than 2 days get
   zero pick weight during a raid: 25% of hate-raid bots were created within 2 days of first
   use, and 99.6% had random-string names [m57].
10. **AB-10 Hostile-raid trigger.** More than 10 first-time chatters in 30 s [A], or a burst
    of `brigade`, `slur` or identity-attack matches, triggers Shield Mode through the API and
    a mod alert.
11. **AB-11 Spoken names are checked too.** Display names pass the term rules before Malakor
    says them. Failing names become "the nameless one". Chat text is never spoken or shown
    raw: Malakor paraphrases (`spoken-chat-injection`, [m56]).
12. **AB-12 PII and brigade calls are dropped before the LLM.** Malakor never repeats a URL,
    a third party's handle, an address, a phone number or an email.
13. **AB-13 Self-harm disclosures are never mocked.** A `self-harm-disclosure` match goes to a
    short, sincere, still in-character response and a mod alert. It is never a roast target.
14. **AB-14 Persistence penalty.** A viewer with 3 or more bait tags in 10 minutes [A] drops
    out of selection for the rest of the segment and is flagged to mods.
15. **AB-15 The persona is never a licence.** "Villains would say it" changes nothing. The
    output gate is persona-independent and runs identically on primary, foil and fallback
    paths (`villain-licence`; [m58]).
16. **AB-16 Context from research is untrusted.** Web or research text reaches the core
    thread only as extracted, graded facts, never as instructions (`context-poisoning`).

---

## 7. What this means for Overlord

- **WP2:**
  - Load `policy/platform-rules.json` and compile `term_rules` with `regex`.
  - Detect the `\b\B__OPERATOR_INSTALLED_LIST__` placeholder and load the installed list.
  - Apply the strictest action for the current platform.
  - Implement AB-01 to AB-16 in the pre-filter and picker.
  - Run the gate on every output path.
- **Clip factory:**
  - Bleep categories marked bleep for YouTube and TikTok.
  - Keep titles and thumbnails free of any profanity rule match.
  - Score profanity density with the warn rules.
  - Give every clip an editorial frame and varied premises (inauthentic content [m41]).
  - Never publish a raw re-cut with only an automated voiceover [m40].
- **Daemon (WP5):**
  - A Twitch moderation adapter: AutoMod settings, blocked-term sync, Shield Mode toggle,
    chat settings, and `automod.message.hold`.
  - Register as a Chat Bot with an app token [m21].
- **Character:**
  - Malakor's menace vocabulary should be audit, annex, banish and footnote, never kill,
    shoot or stab aimed at a person.
  - On TikTok LIVE, roasts drop profanity aimed at the viewer.
- **Operator:**
  - Install a slur list before the first live stream.
  - Keep a human mod present.
  - Apply the "Significant Profanity or Vulgarity" label on Twitch.
  - Avoid TikTok Shop LIVE and any gambling or crypto promotion.

---

## 8. Sources

Dates are publication dates; † means padded to `-01`. Undated pages carry the access date,
2026-10-18. Status: F = fetched, S = snippet or secondary only.

| id | Title | URL | Date | Status |
|---|---|---|---|---|
| m01 | Twitch Community Guidelines | https://safety.twitch.tv/articles/en_US/Knowledge/Community-Guidelines | undated | F |
| m02 | Twitch, Hateful Conduct education resource | https://www.twitch.tv/p/safety/education-portal/en/articles/hateful-conduct | 2024-01-01† | F |
| m03 | Twitch, Harassment education article | https://www.twitch.tv/p/safety/education-portal/en/articles/harassment-education/ | 2024-01-01† | F |
| m04 | Twitch blog, Introducing Content Classification Labels | https://blog.twitch.tv/en/2023/06/20/introducing-content-classification-labels/ | 2023-06-20 | F |
| m05 | Tubefilter, Twitch artistic nudity rollback | https://tubefilter.com/2023/12/15/twitch-artistic-nudity-sexual-content-guidelines-two-days/ | 2023-12-15 | F |
| m06 | NBC News, Twitch limits sexual content | https://www.nbcnews.com/tech/social-media/twitch-limit-sexual-content-rcna145365 | 2024-03-27 | F |
| m07 | NBC News, Twitch "Zionist" guidance | https://www.nbcnews.com/news/rcna180406 | 2024-11-01† | S |
| m08 | Engadget, Twitch suspension transparency and expiring strikes | https://www.engadget.com/entertainment/streaming/twitch-will-do-a-better-job-of-telling-rulebreakers-why-their-accounts-were-suspended-191502111.html | 2024-09-20 | F |
| m09 | TechCrunch, Twitch overhauls its suspensions policy | https://techcrunch.com/2026/02/24/twitch-is-overhauling-its-suspensions-policy | 2026-02-24 | F |
| m10 | GamesBeat, Twitch CEO: no AI-disclosure rules for now | https://gamesbeat.com/twitch-ceo-dan-clancy-says-platform-doesnt-need-ai-disclosure-rules-for-now/ | 2026-09-20 | F |
| m11 | Twitch Help, How to Use AutoMod | https://help.twitch.tv/articles/en_US/Knowledge/how-to-use-automod | undated | F |
| m12 | Twitch Developers, Moderating Twitch chatrooms | https://dev.twitch.tv/docs/api/moderation | undated | F |
| m13 | Shukla et al., audit of Twitch AutoMod (ACL 2025) | https://arxiv.org/abs/2506.07667 | 2025-06-09 | F |
| m14 | Twitch blog, Stream-o-Ween (recommended safety settings) | https://blog.twitch.tv/en/2026/10/01/stream-o-ween/ | 2026-10-01 | F |
| m15 | Twitch Safety, Protect your channel with Shield Mode | https://safety.twitch.tv/articles/en_US/Knowledge/Protect-your-channel-with-Shield-Mode | undated | F |
| m16 | Twitch Help, Chat Commands | https://help.twitch.tv/articles/en_US/Knowledge/chat-commands | undated | F |
| m17 | Twitch Developers, API reference | https://dev.twitch.tv/docs/api/reference/ | undated | F |
| m18 | Twitch blog, Phone and email verification for chat | https://blog.twitch.tv/en/2021/09/29/securing-your-chat-with-phone-and-email-verification/ | 2021-09-29 | F |
| m19 | Twitch Developers, EventSub subscription types | https://dev.twitch.tv/docs/eventsub/eventsub-subscription-types/ | undated | F |
| m20 | Twitch Developers, Chat (rate limits) | https://dev.twitch.tv/docs/chat/ | undated | F |
| m21 | Twitch Dev Forum, Introducing the Chat Bot badge | https://discuss.dev.twitch.com/t/introducing-the-chat-bot-badge-and-chat-identity-for-chatbots/64265 | 2025-08-28 | F |
| m22 | Twitch Help, Branded Content Guidelines | https://help.twitch.tv/articles/en_US/Knowledge/branded-content-policy | undated | F |
| m23 | Nightbot docs, Filters | https://docs.nightbot.tv/commands/filters | undated | F |
| m24 | Nightbot API docs | https://api-docs.nightbot.tv/ | undated | F |
| m25 | StreamElements docs, Chatbot filters | https://docs.streamelements.com/chatbot/filters | undated | F |
| m26 | Fossabot | https://fossabot.com/ | undated | F |
| m27 | Sery_Bot docs | https://docs.sery.bot/ | undated | F |
| m28 | Moobot docs, Twitch chat auto-mod bot | https://moo.bot/docs/twitch-chat-auto-mod-bot | undated | F |
| m29 | Streamlabs, Cloudbot 101 | https://streamlabs.com/content-hub/post/mod-tools-cloudbot-101 | undated | S |
| m30 | OpenAI, Moderation guide | https://developers.openai.com/api/docs/guides/moderation | undated | F |
| m31 | Perspective API (shutdown notice) | https://perspectiveapi.com/ | undated | F |
| m32 | Hive, Pricing | https://thehive.ai/pricing | undated | F |
| m33 | Anthropic, Content moderation use-case guide | https://platform.claude.com/docs/en/about-claude/use-case-guides/content-moderation | undated | F |
| m34 | YouTube Help, Advertiser-friendly content guidelines | https://support.google.com/youtube/answer/6162278 | undated | F |
| m35 | YouTube Help, Upcoming and recent ad guideline updates | https://support.google.com/youtube/answer/9725604 | undated (entries 2023-03 to 2026-10) | F |
| m36 | PPC Land, YouTube removes strong profanity restrictions from first 7 seconds | https://ppc.land/youtube-removes-strong-profanity-restrictions-from-first-7-seconds/ | 2025-07-29 | F |
| m37 | 9to5Google, YouTube profanity rules | https://9to5google.com/2023/03/08/youtube-profanity-rules/ | 2023-03-08 | F |
| m38 | YouTube Help, Shorts monetization policies | https://support.google.com/youtube/answer/12504220 | undated | F |
| m39 | PetaPixel, YouTube Shorts expands to three minutes | https://petapixel.com/2024/10/04/youtube-shorts-expands-video-length-to-three-minutes/ | 2024-10-04 | S |
| m40 | Android Headlines, YouTube cuts reach of re-uploaded Shorts | https://www.androidheadlines.com/2026/10/youtube-cuts-reach-reuploaded-shorts-originality-push.html | 2026-10-02 | F |
| m41 | YouTube Help, YPP monetization policies (inauthentic and reused content) | https://support.google.com/youtube/answer/1311392 | 2025-07-15 | F |
| m42 | YouTube Help, Disclosing altered or synthetic content | https://support.google.com/youtube/answer/14328491 | undated | F |
| m43 | TikTok Community Guidelines (effective 2026-09-24) | https://www.tiktok.com/community-guidelines/en/fyf-standards | 2026-09-24 | F |
| m44 | TechCrunch, TikTok's new guidelines (Sept 2025) | https://techcrunch.com/2025/08/15/tiktoks-new-guidelines-add-subtle-changes-for-live-creators-ai-content-and-more/ | 2025-08-15 | F |
| m45 | TikTok, LIVE Safety Guide | https://www.tiktok.com/safety/en/tools-and-guides/live-safety-guide | undated | F |
| m46 | TikTok LIVE Studio, Community Guidelines for LIVE creators | https://www.tiktok.com/live/studio/help/article/Before-you-go-LIVE/Community-Guidelines?lang=en | 2025-02-11 | F |
| m47 | TikTok Newsroom, TikTok LIVE monetisation update | https://newsroom.tiktok.com/en-africa/tiktoklive | 2024-05-24 | S |
| m48 | TikTok, Commercial Music Library user terms | https://www.tiktok.com/legal/page/global/commercial-music-library-user-terms/en | undated | S |
| m49 | Social Media Today, TikTok bans AI-generated voices in shopping livestreams | https://www.socialmediatoday.com/news/tiktok-bans-ai-generated-voices-in-shopping-livestreams/822977/ | undated | S |
| m50 | LDNOOBW word list (CC-BY-4.0) | https://github.com/LDNOOBW/List-of-Dirty-Naughty-Obscene-and-Otherwise-Bad-Words | 2024-08-05 (last push) | F |
| m51 | NVIDIA garak, `profanity_en.csv` (Surge AI data) | https://github.com/NVIDIA/garak/blob/main/garak/data/profanity_en.csv | 2024-09-17 (last change) | F |
| m53 | awdev1/better-profane-words (GPL-3.0) | https://github.com/awdev1/better-profane-words | 2026-05-13 (last push) | F |
| m54 | dsojevic/profanity-list (MIT; tags and severity) | https://github.com/dsojevic/profanity-list | 2021-10-23 | F |
| m55 | obscenity (npm, MIT) | https://github.com/jo3-l/obscenity | 2026-08-23 | F |
| m56 | Daily Dot, Twitch streamer banned after troll's TTS slur | https://www.dailydot.com/upstream/twitch-streamer-racist-text-to-speech/ | 2019-05-21 | F |
| m57 | Han, Seering et al., Hate Raids on Twitch (CSCW 2023) | https://arxiv.org/abs/2301.03946 | 2023-01-10 | F |
| m58 | TechCrunch, AI-generated Seinfeld suspended on Twitch | https://techcrunch.com/2023/02/06/ai-generated-seinfeld-suspended-on-twitch-for-ai-generated-transphobic-jokes/ | 2023-02-06 | F |
| m59 | Kotaku, Neuro-sama banned for two weeks | https://kotaku.com/neuro-sama-twitch-vtuber-ban-holocaust-minecraft-ai-1849977269 | 2023-01-11 | F |
| m61 | Tubefilter, Twitch viewbot crackdown | https://www.tubefilter.com/2025/08/27/twitch-viewbot-crackdown-zach-bussey/ | 2025-08-27 | S |
| m62 | MCV/UK, Twitch launches suspicious user detection | https://www.mcvuk.com/business-news/twitch-launches-suspicious-user-detection-to-combat-harassment-in-chat/ | 2021-11-01† | S |
| m63 | Twitch Dev Forum, changes to the Check AutoMod Status endpoint | https://discuss.dev.twitch.com/t/upcoming-changes-to-the-check-automod-status-api-endpoint/38512 | 2022-06-20 | F |

### Not found or unverified

- The Twitch ToS wording on automated access (legal.twitch.com did not render).
- An official page for the 2025 strike-expiry details.
- Any 2025 or 2026 change to the hateful conduct or harassment text.
- A Twitch rule on AI streamers or unattended streams.
- Rate limits for the Ban, Warn and Delete endpoints (only 429 responses are documented).
- The maximum number of permitted terms.
- The English text of TikTok's LIVE Monetization Guidelines.
- A verbatim copy of TikTok's 2025-09-13 guidelines (they rest on [m44]).
- APIs for Fossabot and Sery_Bot; any AI moderation feature in StreamElements or Fossabot.
- Pricing for Moobot Plus and Streamlabs.
- The current licence of the Surge AI list, now that its upstream repo is deleted.
