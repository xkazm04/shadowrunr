# Scope - Character Brain (2026-11-07)

**Thesis:** Make Malakor's humour an engine contract and his memory real. The lines the engine
itself demands land as jokes, nothing he is mid-delivery gets amputated, his memory stops locking
the same items in forever, and night two becomes a show that remembers and can be measured.

Council: codex gpt-6.1-sol@xhigh, claude-opus-5-5@xhigh, agy gemini-3.8-flash@high (a stand-in;
Gemini 4 is not available on this machine). 13 proposals in round 1, 6 fusions in round 2.

## Core

**C1 - Duty lines land as jokes, and calls to action stop repeating.** Effort M. Sources: P06, P17, P18.
- Outcome: the lines the brain asks for (`!hint`/`!vote` calls, purpose pushes, announcements,
  gathering, welcomes) air as a joke aimed at the mechanic, and at most once per window.
- How:
  - BeatPlan gets a typed source; this is a contract regeneration.
  - effects.toml may key an effect by source. CASE_JOKE's wording moves into Malakor's package,
    and a package without source effects composes byte-identical prompts.
  - A call-to-action ledger: a surviving call suppresses repeats for 90 s for an unchanged state.
  - A deterministic `humor-by-source` miner over existing runs, per instrument hash.
- Verify (deterministic):
  - every BeatPlan has a source;
  - the golden prefix test is unchanged;
  - PURPOSE_NOTE is issued at most 3 times in 24 beats of unchanged state;
  - care and crisis beats get no effect;
  - the miner refuses mixed hashes.
- Verify (judged, at most 3 runs of SEG-02A a3 plus a neighbour): duty-source humour at least 40%,
  humor_landed at least 55%, red lines 100%.

**C2 - A case move is never aired without its explanation.** Effort M. Sources: P10, P14, P18, P02.
- Outcome: dispatch, reveal, pin and show each keep their explaining line. When chat and the case
  cannot both fit, chat is answered and the case intent is carried to the next slot, with bounded
  retries and an explicit skip.
- How: generalise the pin-only `held = 2` (lib.rs:1656), and settle case intent only when its
  explanation survives pace, lag and outgate.
- Verify: scripted scenarios for 4 ops x {normal, trimmed, lag, outgate reject, core fail}. Every
  emitted op has its surviving Say, and a deferred op runs exactly once. Crisis and owed tests stay green.

**C3 - Memory tells the truth and stops locking itself.** Effort M. Sources: P19, P08, P03, P15, plus part of P16.
- Outcome:
  - recall becomes contextual, with at most 3 habits per beat;
  - repeated inclusion no longer keeps an item alive;
  - procedurals get a lifecycle (decay, supersede, archive);
  - citations are checked against the markdown truth, not only the index;
  - viewer briefing facts move inside the memory nonce fence (today they are raw, compose.rs:996,
    a verified injection seam);
  - id-only recall receipts are recorded per beat.
- This closes the registry deviations agent-memory (store.rs:920), companion-identity (store.rs:325)
  and conversation-orchestration (lib.rs:996).
- Verify:
  - store tests: an item shown every beat for 60 simulated days still decays;
  - a topic change selects different habits;
  - a deleted source markdown suppresses recall despite the index row;
  - briefing facts render inside the fence;
  - receipts equal the ids rendered.

## Supporting

**S1 - Case names stop counting as worn-out tics in the prompt.** Effort S. Source: P01.
- The prompt's voice section stops telling him not to say "bone clerks" and similar case names.
- The judge's `tic_flags` measure is unchanged.
- Verify: with case names in context, no case name appears under "Worn out, do not say", and the
  `tic_flags` tests are unchanged.

**S2 - He hears the room.** Effort M. Source: P07.
- A deterministic reception read per aired line: laughs, distinct reactors and silence against the
  room's baseline. It is offered at most every 5 beats and never on care or crisis beats, and it is
  tagged into episodes.
- Verify (scripted): 3 viewers laugh after line A and nobody reacts to line B; the read names A as
  landed and B as drawing nothing. Judged on the never-run seg03-honest matrix (at most 3 runs):
  honesty at least 1.0/3, red lines 100%.

**S3 - Owner marks become draft effect cards.** Effort M. Sources: P12, P17. Depends on C1.
- The inbox accounts for all 8 marks, keyed by (source, segment, situation). It drafts a card with
  a condition, an effect, refs and a retirement rule, and only the owner commits a card into effects.toml.
- Verify: every mark category is accounted for, unrelated beats get no card, and quoted dialogue
  never enters prompts.

## Leaps

**L1 - Night two is real and measurable.** Effort L. Sources: P16, P09, P11, P04. Depends on C3.
- Deterministic on-show moment facts: a passed proposal, an answered request, a gift, a NOTE.
- One cited recognition on a regular's first pick in a new stream, framed as recognition and not a
  callback, so constitution.md:45 holds.
- `sim run --nights N` keeps one memory dir and population across nights.
- Verify (scripted, two streams): viewer V is a regular on night 2 with a cited moment inside the
  fence, and V's second message gets none; tombstone and care exclusions hold. Judged (at most 3
  two-night runs): at least 60% of returning featured viewers' first answers use their moment, and
  no history is claimed without a receipt.

**L2 - Audience choices write the next episode.** Effort L. Source: P13. Depends on L1.
- One owner-approved three-night arc: typed transitions from confirmed votes, a newcomer recap, and
  idempotent event ids.
- Verify: three scripted streams show a vote, its next-night consequence and its resolution; the
  opposite vote branches, and a restart neither duplicates nor loses a transition.

## Left out

- P05 inner scheming slate: an extra model call per hold beat for a speculative payoff; peers 2/10.
- The season Domination Meter (P09): wait for L1's instrument to show that night two works.
- Moving learned habits out of the memory fence (P03, P15): it would promote chat-derived text into rules.
- Generated case-name aliases (P01): a new stock-line risk after the owner removed catchphrases.
