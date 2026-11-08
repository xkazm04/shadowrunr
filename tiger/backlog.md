# Tiger backlog

## From 2026-11-07-character-brain ([run](hunts/2026-11-07-character-brain/outcome.md))

**Owner decisions**
- Approve, edit or drop `characters/malakor/arcs/DRAFT-council-object.json` (L2). No arc runs
  until an approved one exists.
- Review the new duty effects in `characters/malakor/effects.toml` (C1: purpose, cta, room_event,
  and case_move, which moved from the engine's CASE_JOKE). This change gives a new instrument hash.
- Spend for the judged checks, at most 3 targeted runs each:
  - C1: duty-source humour at least 40%, humor_landed at least 55%, on SEG-02A a3 plus a neighbour;
  - L1: at least 60% of returning viewers recognised, on 2-night SEG-01 a30;
  - S3: per card, once one exists.

**Kept, not chosen**
- S2 He hears the room (P07): a deterministic reception read so his "that joke died" claims are
  true; targets SEG-03 honesty at 0.42-0.64/3.

**Deferred inside landed items**
- L2: replace room.rs state-text vote parsing with typed outcomes (42 SegmentContext literals).
- L2: propose-outcome arc triggers.
- The season Domination Meter (P09): after L1's instrument shows night two works.

**Found**
- The `shadowrunr-engine` `fake_cli` timing race
  (`a_chat_pick_gets_in_while_a_warm_core_holds_a_slot_and_judges_fill_the_batch_lane`).
