# Verdict - Character Brain hunt (2026-11-07)

Judge: claude-opus-5-5, the caller session. It read pool.md blind and verified each premise at
its anchors at 24039f7. The judge shares a family with one seat; the seats were mapped only after
this file was written.

## Triage

Score = value x confidence / effort weight (S 1, M 1.6, L 2.5).

| id | premise | already | value | conf | effort | fit | score | peers | note |
|---|---|---|---|---|---|---|---|---|---|
| P06 | true | CASE_JOKE covers case moves only (case.rs:77) | 5 | 4 | M (seat S) | strong | 12.5 | 9 | Duty notes PURPOSE/ANNOUNCE/GATHERING exist with no comic contract (orchestrate/mod.rs:216,263,275). The mined 4/77 and 7/21 figures are the seat's, not re-run; the case.rs:70-76 comment corroborates the mechanism. |
| P17 | true | - | 5 | 4 | M | strong | 12.5 | - | Fusion of P06+P12. It beats P06 on mechanism: package-owned effects keyed by beat source, not a second string system. Adopted into C1. |
| P10 | true | pin-only hold (lib.rs:1656) | 4 | 4 | M (seat S) | strong | 10 | 8 | Closes BATCH-1 §4 lead "case lines lost to pace trim". |
| P14 | true | - | 4 | 3 | M | strong | 7.5 | - | Fusion P02+P10. Its "never prune line 2 of any 2-line joke" ignores the char budget; only the intent-carry part is kept. |
| P18 | true | - | 4 | 3 | M | strong | 7.5 | - | Fusion P02+P06+P10. The CTA cooldown starts only from a surviving invitation; that detail is kept. Grammar unit metadata is dropped as too heavy. |
| P02 | partly | - | 3 | 2 | S | ok | 6 | 4 | "held=2 whenever chat" overruns the model's own budget. The punchline-amputation claim is not measured. Folded into C2. |
| P19 | true | - | 4 | 4 | M | strong | 10 | - | Fusion. The best memory spec: exposure never preserves an item, receipts are ids only, and learned content stays fenced. |
| P08 | true | - | 4 | 4 | M | strong | 10 | 8 | Lock confirmed: the always-tier is mark_recalled every beat (lib.rs:1336) and decay keys on last_recalled_ms (store.rs:921). |
| P03 | partly | - | 3 | 3 | S | ok | 5.6 | 4 | The decay part is right. **Rejected part:** moving consolidated habits out of the nonce fence would give text derived from chat the authority of rules, against cited-memory-with-tombstones. |
| P15 | partly | - | 3 | 3 | M | ok | 5.6 | - | Same unfencing flaw as P03; otherwise duplicates P19. |
| P16 | true | - | 5 | 3 | M-L | strong | 7.5 | - | Fusion P09+P11+P08. Deterministic moment writes (consolidate.rs:447 is the only production writer), a two-night instrument, and a framing that keeps constitution.md:45. **Verified defect:** briefing facts are joined raw outside the memory fence (compose.rs:996, lib.rs:1309). |
| P09 | true | - | 5 | 2 | L | strong | 4 | 6.5 | Season meter and Grudge Ledger. It honestly flags the constitution "tonight" clause. The seasons part is deferred. |
| P11 | true | - | 4 | 3 | M | strong | 7.5 | 7 | Folded into L1 via P16. |
| P04 | true | - | 3 | 2 | M | ok | 3.75 | 4.5 | Stance labels such as "insolent rebel" risk targeting people. Superseded by P16. |
| P07 | true | - | 4 | 3 | M | strong | 7.5 | 6.5 | The tone asks for "which joke landed" (unhinged-roastmaster.md:46-48), but the brain has no signal. The seat reports that sim emotes are random, so in sim it buys honesty, not humour. |
| P12 | true | inbox admits 4/8 tags (inbox.rs:18-21) | 3 | 3 | M | ok | 5.6 | 5 | Becomes S3 on top of C1's source-keyed effects. |
| P01 | true | - | 3 | 4 | S | ok | 12 | 3.5 | leftovers.md:43 states the cause. **Condition:** exempt case names only in the prompt's voice section; `tic_flags` (the judge's measure) stays unchanged, since the judge is never made easier. Generated alias rotation is dropped. |
| P13 | true | - | 4 | 2 | L | ok | 3.2 | 4.5 | Audience-authored arc. It needs L1's persistence first. |
| P05 | true | - | 3 | 1 | L | weak | 1.2 | 2 | An extra model call per hold beat, with a speculative payoff. Dropped. |

## Clusters

- **Humour as an engine contract:** [P06 lead, P17, P12-part, P18-cta]. The two seats that saw it
  independently are P06 and P12; P17 and P18 fused them.
- **Delivery unit:** [P10 lead, P14, P18, P02]. All three seats arrived here.
- **Memory truth and freshness:** [P19 lead, P08, P03, P15]. All three seats.
- **Night two:** [P16 lead, P09, P11, P04]. All three seats.

Round 2 fusions beat their parents in the judge's scoring in 3 of 6 cases: P17 over P12, P19 over
P03, P16 over P09 and P04. P14 did not, because it reintroduced P02's budget overrun.

## Dropped

- P05: speculative payoff, an extra model call per hold, peers 2/10.
- P04: superseded by P16; its stance labels risk aiming at people rather than acts.
- The unfencing parts of P03 and P15: they promote remembered chat into policy.
- P01's alias rotation: it generates in-world aliases at runtime, which is new stock-line risk;
  the owner removed catchphrases.
