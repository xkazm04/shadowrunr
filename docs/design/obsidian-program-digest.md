# The Obsidian Program - Build Digest

Source: `.contest/arena/overlord-directions-fuse/runs/visual/A-1-text.txt` (entry A, variant 1). Tags in the source: [measured], [verified], [assumption], *est.*, [unverified]. Line refs are to that file.

## 1. Malakor bible essentials
- **Identity:** Arch-Lich Malakor, High Auditor of the Obsidian Spire. Bone-white and verdigris-bronze hollow brass ribcage, charcoal chancellor robes, Cracked Obsidian Geometer's Crown (floating astrolabe shards), Phylactery Orb (green-glass ledger sphere that pulses with TTS amplitude). Clean-room from Mattel Skeletor. Forbidden terms: Skeletor, He-Man, Grayskull, Eternia, Myah, purple hood, ram-skull staff.
- **Want (`core_want`):** prove mortal institutions incompetent via superior auditing and tools. "Audit & Annex". Earth is a fraud-ridden bankruptcy estate. Scammers are rival overlords. Viewers are "Conscripted Auditors of the Realm". Catchphrase: "Let the deficit be recorded."
- **Voice:** designed from prose via Qwen3-TTS 1.7B VoiceDesign, 5 mood clips (gloat, sneer, rage, whisper, cackle), spoken live by VoxCPM2 compiled. Pedalboard chain: -3 st, formant -1.5, light bitcrush, 80 Hz HPF, crypt IR, -14 LUFS (`obsidian_crypt_v2`). `consent_doc_uri = 'designed:not_applicable'`.
- **Craft Canon:** frozen voice settings. Text carries emotion. At most 1 hesitation mark per line, 1-2 short sentences per turn. Foils get 1 sentence per turn. Invented nouns go in `world_lexicon.json` (target 0% WER on Whisper large-v3-turbo).
- **Red lines:**
  - Only Malakor may comment on real public figures.
  - No real-person name combined with a synthesized voice or likeness.
  - No copyrighted characters.
  - No real crypto or stock promotion.
  - Subjects are public-record only.
  - Nothing below DOCUMENTED_PRIMARY is stated as fact.
- **Memory model:**
  - Frozen Bible: 8,192-token immutable prefix, 1h cache TTL, shared by both lanes.
  - `overlord-memory` MCP: SQLite plus FTS5, 8 tools. It holds `character_bible_versions`, `minion_dossier`, `season_campaign_arc`, active cases, ranks and releases.
  - Session recycle at every 25-45 min segment break instead of auto-compaction (`--autocompact 500k`), re-hydrated in about 120 ms.
  - Persona Drift Sentinel: at most 2 sentences and 180 chars per turn, one ellipsis, no helper-bot cliches, cosine similarity of at least 0.80 against 50 Golden Villain Lines, re-anchor on drift.
  - Nightly Batch API (50% off) distills 5 canon memories plus a Grudge Graph, updates the Domination Meter and drafts the next Beat-Deck.
  - Season: "The Great Audit of Earth", 12 weeks, Act I wk 1-3, Act II wk 4-8, Act III wk 9-11, wk 12 finale. Goal 6 sealed dossiers, 1,500 OSM tiles, 3 OSS releases. A miss triggers an on-air "Demotion Trial" voiced by the 5 foils.
- **`minion_dossier` fields:** `viewer_id, display_name, imperial_rank (UNSWORN_THRALL->THRALL->INQUISITOR->HEX_CAPTAIN->GRAND_AUDITOR), fraud_hints_accepted, osm_tiles_validated, foundry_prs_credited, running_gag_note, last_seen_episode`.

## 2. Segment portfolio loop
- **IDs:** SEG-01, SEG-02A, SEG-02B-I (Cartography), SEG-02B-II (Dependency Doctrine), SEG-05 (Celestial Tribunal, Zooniverse, 15-min PILOT, consensus at least 75%), SEG-00 (RETIRED).
- **Lifecycle:** PILOT (15m) -> TRIAL (20-25m, 3 runs over 2 weeks) -> REGULAR (25-35m) -> FLAGSHIP (40-55m) -> RETIRED. A "throw" is an in-character 60s "Imperial Purge".
- **SES:** `0.30·Z(chat_msgs_min) + 0.25·Z(poll_vote_rate) + 0.25·Z(ccv_retention_delta) + 0.20·Z(clip_48h_watch_pct)`, computed against a 14-day rolling mean.
- **Thresholds [assumption]:**
  - PILOT->TRIAL at SES >= 0.0.
  - TRIAL->REGULAR at SES >= +0.4 over 3 runs.
  - ANY->RETIRED at SES < -0.5 over 3 runs.
  - Live reflex: extend +10m if SES > +1.2 SD. Cut -10m and hand over if SES < -0.8 SD for 5 min.
  - Calibrate over the first 8 streams (Season 0).
- **Clip funnel score (T+48h):** `0.5·watch_through + 0.3·shares_per_1k + 0.2·follows`.
- **Approvals:**
  - Automatic in-stream: extend/cut and handover.
  - Automatic nightly batch: recompute SES, draft airtime.
  - Owner one-click Morning Sheet: promote/retire.
  - Strictly owner-gated: new allowlist domain, unsealing a named fraud case, pushing to a public release branch.
- **Tables:** `segment_portfolio(segment_id, display_title, lifecycle_stage, default/min/max_minutes, rolling_14d_ses, consecutive_low_runs, requires_publish_gate, owner_locked_status)` and `segment_telemetry_runs(... computed_ses, auto_reflex_action NONE|EXTENDED_10M|TRUNCATED_HANDOVER)`.

## 3. SEG-01 Improvised Room
- 20-30 min opener, FLAGSHIP. The cold-start engine: Malakor plus a rotating original parody foil. Duet Beat-Deck fires a new premise whenever chat is quiet for more than 12s. Every chatter is greeted by name and made a "Founding Inquisitor".
- **Foils (voice):**
  - FOIL-01 Braxton Vance, smug tech billionaire (Kokoro 0.09s, 65/35 am_adam+bm_george blend, speed 1.08).
  - FOIL-02 Col. Brock Sterling, action hero (Chatterbox, RTF 0.48).
  - FOIL-03 Inspector Benoit Graves, detective (Kokoro).
  - FOIL-04 Seraphina Glow, life coach (Kokoro).
  - FOIL-05 Klank-9 / WARDEN-0, compliance homunculus (Kokoro). It triggers the "DECREE REDACTED BY TRIBUNAL" gag on filter hits.
- **Per-foil red-line JSON:** `voice_id, origin_type, consent_doc_uri, blocked_proper_nouns[], real_person_commentary_rule`.
- **Summon:**
  - 0-5 viewers: the scheduled foil is drawn automatically.
  - 5+ viewers: a 60s poll with `!summon vance|sterling|graves`.
  - Channel-point redeem "Swap Foil Mid-Scene".
- **Duet generation:** one `claude-haiku-5-5` call (thinking disabled, frozen Bible plus foil spec) returns `[{spk:"FOIL",text},{spk:"MALAKOR",text}]`. Audio mutex: foil on Kokoro, 180ms breath, Malakor on VoxCPM2. No overlapping TTS.
- **Handover:** `ccv >= 15_sustained_3m OR elapsed_min >= 25` [assumption; A/B 15m vs 25m ceiling over weeks 1-4]. The foil dares Malakor ("show me one actual fraudulent syndicate"). OBS switches to `HEX_INVESTIGATION_MAIN`. The foil shrinks to a "Skeptical Observer" badge that chimes in about once every 10 min. Scenes: SCENE_INTRO_SEAL, SCENE_IMPROV_DUET, SCENE_HEX_INVESTIGATION, SCENE_FOUNDRY_IDE, SCENE_OSM_LEGION, SCENE_OUTRO_LEDGER, BRB_SEAL (panic).

## 4. SEG-02A Hexmap Investigation (flagship, 40%)
- **App:** hex grid (HEX-01..HEX-19, axial q,r), fog of war, side inspector per hex. Dual bar: Malakor Confidence vs Minion Jury Rating. A divergence over 25% turns the hex amber "DISPUTED NODE" and forces a re-audit. A Corrections and Right-of-Reply drawer sits in the header. Overlay lock "ALLEGED". Example case `S01-C03-AETHERYIELD` (public FTC/SEC record).
- **Grades:** DOC=DOCUMENTED_PRIMARY (emerald), REP=REPORTED_PRESS (cyan), COMMUNITY_CLAIM/CLM (amber), SPC=OVERLORD_SPECULATION (violet). `alleged_language_lock` is generated as `evidence_grade != 'DOCUMENTED_PRIMARY'`.
- **Code:**
```sql
investigation_cases(case_id, codename,
  subject_category IN ('PUBLIC_ENFORCEMENT_ENTITY','REGULATOR_WARNED_SCHEME','COURT_RECORD_PUBLIC_FIGURE'),
  primary_public_record_url NOT NULL, arc_session_number DEFAULT 1,
  case_status IN ('CALM_RESEARCH','LIVE_ACTIVE','DOSSIER_SEALED','RETRACTED'),
  owner_approved_for_stream DEFAULT 0, authority_submission_ref)
hexmap_nodes(hex_id, case_id, axial_q, axial_r,
  node_type IN ('ENTITY','DOMAIN_ARCHIVE','FILING_DOC','ONCHAIN_WALLET','CLAIM_HYPOTHESIS'),
  fog_state IN ('FOG_LOCKED','CALM_STAGED','REVEALED_LIVE','DISPUTED'),
  evidence_grade IN (4 above), alleged_language_lock GENERATED,
  pii_scrub_verified CHECK=1, sanitized_summary, wayback_or_filing_hash,
  crowd_confidence_score DEFAULT 0.5, crowd_next_lead_votes DEFAULT 0)
corrections_and_reply_log(entry_id, case_id, hex_id,
  entry_type IN ('FACTUAL_CORRECTION','RIGHT_OF_REPLY_STATEMENT','GRADE_DOWNGRADE'),
  public_note, blocks_clip_publishing DEFAULT 1, logged_at_utc)
```
- **Research agents:** Claude Code CLI in the isolated browser box during CALM phase (off-stream). Allowlist: ftc.gov, sec.gov, fca.org.uk, dfpi.ca.gov, justice.gov, OpenCorporates, Wayback. Page text is wrapped in `<untrusted_web_evidence>` and parsed by a tool-less extraction schema. A PII scrubber (regex plus NER) runs before DB insert. Output is staged hexes (CALM_STAGED). Live phase only narrates, reveals and takes votes. Arc: Session 1 (anomaly and filing map) -> Session 2 (shell and archive trail) -> Session 3 (dossier sealed, then a detector rule built in 2B-II).
- **Six guardrails:**
  1. Public-record subjects only.
  2. Four-tier grades.
  3. Zero crowd action (no contact info shown, URLs and "raid/review bomb/call" deleted pre-LLM).
  4. Corrections log plus Owner Publish Gate.
  5. Isolated browser plus prompt-injection guard.
  6. Owner-only authority reporting (Evidence Packet to FTC, Action Fraud or registrar; ticket receipt ID logged).
- **Audience commands:** `!hint <hex_id>` (nominates the next fog hex), `!grade <hex_id>` (rates evidence). The UI also shows "+Upvote" and `!challenge Grade`.

## 5. SEG-02B (being descoped; only the dependencies to cut)
- **2B-I Cartography Legion:** viewers map real HOT Tasking Manager tiles in their own OSM accounts (`!enlist <osm_handle>`). Hashtag `#OverlordCartographyLegion`, OSM Organised Editing Guidelines wiki page. Points only when VALIDATED (`buildings_added_count*10`). Polls `/api/0.6/changesets` every 30s. The agent never edits OSM. Table `osm_legion_contributions`.
- **2B-II Dependency Doctrine:** Claude Code builds PhylacteryGuard (anti-scam detector fed by 2A patterns) and LichReader. Chat polls pick the feature. `!issue` files cases. Credits go to CHANGELOG.md. The agent pushes only to `staging/ep-XX`. The owner signs the release with a YubiKey. osv-scanner, `--ignore-scripts` and a PreToolUse npm block. Table `dependency_doctrine_releases` (FK `linked_fraud_case_id`).
- **Cross-references to strip when descoping:** season goals (osm_tiles_*, oss_releases_*), `minion_dossier.osm_tiles_validated/foundry_prs_credited`, the Act III run-of-show, 35% of airtime, the two clip styles, the Month-1 PhylacteryGuard milestone, and SEG-05.

## 6. Runtime
- **Lanes:**
  - FAST: `claude-haiku-5-5` via the Messages API with `thinking: disabled`, 12k cached prefix, about $0.15-0.35/hr. Target 0.8-2.5s *est.* Local fallback Gemma 4 26B-A4B Q4 on the 4090.
  - SLOW: Claude Code CLI `claude -p --input-format stream-json --output-format stream-json`. It runs the director, research and coding, with MCP `overlord-memory` (`--strict-mcp-config`), PreToolUse/PostToolUse domain-allowlist hooks, and is recycled per segment.
- **Auth/lane modes:**
  - Mode 1 (recommended hybrid): slow lane on monthly plan, fast lane on Haiku API key (about $15-35/mo).
  - Mode 2 (zero API spend): warm CLI haiku with `--effort low`, plus local Gemma during bursts.
  - Mode 3: `CONSOLE_API_KEY` with `--bare`, both lanes.
  - `EngineAuthMode = 'SUBSCRIPTION_PLAN_OAUTH' | 'CONSOLE_API_KEY_BARE' | 'LOCAL_4090_FALLBACK'`, class `SwappableEngineAdapter` in `auth-adapter.ts`, `hotSwapOnQuotaOrPolicyAlert()`. Claimed 200 ms swap. Auto-swap when the 5h window is over 80% used. Plan auth uses `claude setup-token` with `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`.
- **Pre-flight (obs-websocket StartStream interlock, all 8 must be green):**

| ID | Check | Pass bar |
|---|---|---|
| PF-01 | Claude Code CLI v2.1.302 plus plan headroom | handshake < 2,500 ms, cache TTL = 1h |
| PF-02 | Auth adapter plus standby API key | armed |
| PF-03 | VoxCPM2 Triton-compiled | RTF < 0.65 |
| PF-04 | Cast consent ledger | no NULL `consent_doc_uri` |
| PF-05 | three-vrm renderer | 60fps, JawOpen/EyeGlow bound |
| PF-06 | OBS 32.1.x, dual NVENC, Aitum 9:16 | 15s stream delay locked |
| PF-07 | Isolated browser box | Playwright MCP, DNS allowlist, zero cookies |
| PF-08 | Moderation canary | injects a banned vector across primary and fallback, 100% block |

- **Moderation (3 layers):**
  - Input: slur/PII/doxx regex, URL and shell stripping, anti-brigade. Raw chat never reaches the slow lane. 1-5 CCV gets instant replies. 50-300 CCV uses 15s windows, MiniLM clustering and polls, emitting typed JSON intents.
  - Output: `emitModeratedSpeech()` / `runPathIndependentSafetyGate`, compiled into `@kazm/voice-tts` and the OBS emitter, covering primary, foil and fallback paths. It requires an "alleged" qualifier for SEG-02A below DOC and checks foil blocked nouns. A block triggers `triggerRedactionShutter` and a WARDEN-0 gag.
  - Panic: 15s OBS delay plus the `BRB_SEAL` scene.
- **Browser box:** second notebook with headed Chromium via Playwright MCP, never Claude-in-Chrome. OS DNS/proxy allowlist (ftc.gov, web.archive.org, tasks.hotosm.org). Audio muted, downloads and POST forms blocked. Video goes to the desktop OBS over NDI with an NSFW/PII classifier. Reason given: Windows 11 Home has no Hyper-V sandbox.
- **Stack:** RTX 4090 24GB. VoxCPM2 7.8GB, Kokoro 1.0GB, Chatterbox-Turbo 3.6GB. Peak VRAM 15.2-18.4GB est. Rehearsal gate: VRAM <= 20.5GB, dropped frames < 0.2%.

## 7. Clip factory and publish gate
- Aitum Vertical 1.6.4 records native 1080x1920. Window = peak - 12s pre-roll. Duration 15-60s.
- **Score:** `0.35·AgentMarker + 0.25·ChatZ + 0.15·AudioRMS + 0.25·HaikuHookScore`. Keep the top 5 per episode. Markers: FOIL_ROAST, HEX_REVEAL, RELEASE_ANNEXED.
- **Captions:** from the exact TTS script alignment (drift <= 40 ms), not ASR. ffmpeg h264_nvenc, libass, loudnorm. faster-whisper large-v3-turbo handles external audio.
- **Styles:** Seg1 split-stage 25-35s. Seg2a 45-60s with an ALLEGED header and a burned-in source citation. Offline Vault Dispatch 35s (Qwen3/IndexTTS2).
- **Gate (`clip_candidates`):**
  - `requires_mandatory_gate` is generated as `linked_case_id IS NOT NULL OR segment_id='SEG-02A-HEXMAP'`.
  - `gate_evidence_badge_burnt`, `gate_no_open_corrections`, `owner_publish_signature`.
  - `publish_status` runs RENDERED_DRAFT -> PENDING_OWNER_GATE -> APPROVED_FOR_UPLOAD -> PUBLISHED, or REJECTED.
  - 4-point checklist: citation shown, ALLEGED plus grade badge, zero PII or brigade call, no open correction.
- **Upload router:**
  - YouTube `videos.insert` (100/day bucket, `containsSyntheticMedia=true`).
  - TikTok Inbox draft pre-audit, Direct Post (at most 15/day) post-audit.
  - Twitch Helix Create Clip.
  - File both API audits on Week 1 Day 1.
  - A human hook review is kept as a defence against the "inauthentic content" policy.
- **Platform TTS/banned-words:** the report has no platform-specific banned-word list or Twitch AutoMod/TTS integration. See gaps.

## 8. Showrunner OS
- **Packages (Turborepo `@showrunner/*`):** `segment-engine`, `character-bible`, `preflight-gate`, `clip-factory`, `hexmap-evidence`.
- **SKUs:**
  - SKU1 Foil Co-Host and Invasion widget (BYO key).
  - SKU2 Hexmap and Writers'-Room Studio.
  - SKU3 Supervised Talent Pods or Studio Seats.
- **Tenancy tables:**
  - `showrunner_tenants(tenant_id, organization_name, sku_tier, anthropic_billing_mode TENANT_BYO_CONSOLE_API_KEY|MANAGED_RESALE_SALES_APPROVED, sandbox_vm_id, outbound_counter_lock, monthly_spend_cap_usd=250, kill_switch_tripped)`.
  - `cast_voice_consent_ledger(... voice_origin, consent_doc_uri, spoken_consent_wav_hash, render_enabled GENERATED)`.
  - `supervised_outbound_audit_log(... proposed_action_type BROWSER_NEW_ORIGIN|PUBLISH_NAMED_CLIP|GUEST_INVASION_START, operator_signature)`.
- **Isolation and terms:** Firecracker/gVisor per tenant, per-tenant egress allowlist proxy, spend caps. Agent SDK and Messages API only, never consumer OAuth for tenants. Branding "{Character} Powered by Claude". Standard on three-vrm (MIT) because of the Live2D exclusion.
- **Borrowed ideas:** B-3 Book-a-Villain with host BANISH, B-2 nightly Writers'-Room beat sheets, C-3 Outbound=0 and signed audit, A-2 foil SDK and duet bus, A-3 WARDEN pods, B-1/C-1 season kits.
- **Sequencing rules:** no forked code in months 1-2; 4-week flagship soak before exposure; platform work <= 20% of time.

## 9. Plan, cost, risks
- **Plan:**
  - Week 1: 10-candidate tech-spike (Qwen3 VoiceDesign, Pedalboard, VoxCPM2, Chatterbox, Kokoro, faster-whisper, IndexTTS2, PixiJS PNGtuber+, three-vrm via Tripo, Leonardo). Wire the Pre-Flight Board and auth adapter. 2h unlisted rehearsal: 0 leaks, RTF < 0.60.
  - Month 1: 3x/week 2h shows, three-vrm swap, PhylacteryGuard v0.1.0, first dossier, SES calibration. Gates: retention >= 65% across the Seg1->2A handover, operator load <= 4.5 h/week.
  - Month 3: Season 1 done, first promote/retire cycle, Direct Post, Showrunner alpha with 5 partners (Anthropic sales check).
  - Month 6: Season 2, 10+ dossiers, 5,000+ tiles, GA, operator load <= 3.5 h/week.
- **Cost (est. +/-50%):**

| Mode | 24h/mo | 120h/mo |
|---|---|---|
| Plan primary (fast lane on Haiku API) | $29-62 | $63-117 |
| API failover, budget | $48-108 | $240-540 |
| API failover, quality | $180-216 | $900-1,080 |
| API failover, premium | $384-480 | $1,900-2,400 |

  Tripo is about $1.50 one-off. Leonardo about $10/mo.
- **Top risks (all High unless noted):**
  - R-01 plan Terms/quota.
  - R-02 defamation.
  - R-03 brigading/doxxing.
  - R-04 likeness/Mattel IP.
  - R-05 prompt injection.
  - R-06 filter bypass on a fallback path.
  - R-07 (Medium) silent Triton eager fallback.
  - R-08 (Medium) clip-upload audits and inauthentic-content policy.

## 10. Gaps and contradictions
1. **ID drift:** `SEG-01-IMPROV` vs `SEG-01-IMPROV-ROOM`; `SEG-02A-HEXMAP` vs `SEG-02A-HEXMAP-INVESTIGATION`; `SEG-02B-I/II` vs `SEG-02B1-OSM` / `SEG-02B2-FOUNDRY`. SEG-05 and SEG-00 are absent from the schema comment. The mandatory-gate SQL hardcodes `'SEG-02A-HEXMAP'`.
2. **Retire threshold:** < -0.5 SD x3 runs for TRIAL, but SEG-00 "retired at < -0.8 SD" (the live-reflex value).
3. **Airtime:** table says 25/40/20/15%. The sample night gives 2B-II 27m and 2B-I 23m, which is out of order with the shares. SEG-01 is "FLAGSHIP" at 25m while FLAGSHIP is defined as 40-55m.
4. **Chat commands:** guardrail 3 says "only !hint and !grade", but `!summon`, `!enlist`, `!issue` and `!challenge` exist. The run-of-show says `!lead`. The filter keyword "call" would false-positive on normal chat. `!hint hex6` vs `<hex_id>` = HEX-06.
5. **Schema/JSON mismatch:** the UI hex JSON has `language_qualifier_required`, `crowd_action_block` and `owner_publish_gate`, none of which are columns. The grade enum shows "COMMUNITY_CLAIM" (badge CLM) and `fog_state DISPUTED` overlaps with the SPC grade.
6. **Ranks:** `minion_dossier` ranks vs "Surveyor Thrall->Hex-Captain->Grand Geometer". The ledger is `cast_voice_ledger` in PF-04 vs `cast_voice_consent_ledger`. The designed-voice URI is `designed:not_applicable` / `designed:n/a` / `designed:not_applicable_kokoro_blend`.
7. **Subject scope:** `COURT_RECORD_PUBLIC_FIGURE` and OpenCorporates/Wayback sit outside the stated "public-record regulator domains" gate. Everything below DOC is a "claim", yet only DOC hexes are fact. No rule says who may be named inside a filing (officers vs individuals).
8. **Fast-lane thinking:** the report says the CLI cannot disable thinking, yet Mode 2 runs the fast lane through a warm CLI haiku worker with `--effort low`. The "$0" claim ignores this.
9. **Terms risk:** the report admits consumer-plan unattended use is barred, yet makes it the primary mode and gives `claude setup-token`. Flags such as `--permission-prompts none`, `--system-prompt-snapshot`, `--autocompact`, `--include-hook-events` and `--bare` are unverified. PF-01 should check them against `claude --help`.
10. **Hot-swap:** "200 ms" vs `recycleSlowLaneAtSafePoint()`, which waits for a safe point. `LOCAL_4090_FALLBACK` is in the type but not implemented in the code.
11. **VRAM:** FOIL-02 uses Chatterbox (3.6GB). VoxCPM2 + Kokoro + Chatterbox = about 12.4GB, over the Kokoro-spike bar of "combined TTS <= 9.0GB". The foil cast is not cleanly Kokoro-only.
12. **Cost:** Mode 1 says $15-35/mo total vs the table's $29-62.
13. **Prompt prefix:** 8k (Bible) vs 12k (fast-lane cached prefix).
14. **Remote runtime:** it does not say how the desktop Claude Code reaches Playwright on the second notebook (MCP transport, auth, network). It does not say where the allowlist proxy runs.
15. **Platform banned words:** there is no Twitch/YouTube banned-term list, AutoMod interaction, or platform-TTS policy. The output gate covers only slurs, PII and unbadged accusations. The 15s delay and BRB_SEAL are the only platform-ban backstop.
16. **Clips:** SEG-01 is "auto-published after hook check" in one place and "owner picks 1-2 takes/night" in another. The offline Vault Dispatch uses Perth watermarking, while `containsSyntheticMedia` is only mentioned for YouTube.
17. **Operator load:** 3 shows/week x 2h plus a 4.5 h/week prep target is unrealistic if the owner must be present for the gates.
18. **Hex data:** the fabricated example case "AetherYield" and "Case #26-CV-0412" are demo data. Do not ship them as real. Naming real filings requires the owner gate.
19. **Dates:** Character.AI Chapter 11 is given as 2026-10-13 and as "10-12/13".
