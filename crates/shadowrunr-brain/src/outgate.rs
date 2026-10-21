//! One output gate for every surface a line reaches.
//!
//! A line the brain produces reaches the audience or memory through one of five surfaces:
//! the host's own voice, a foil's voice, an on-screen card, a package stall line, or a NOTE that
//! becomes a long-term episode. Each surface declares its [`Profile`] here, the checks it runs,
//! and [`pass`] runs them in one fixed order. A rail enforced on four paths out of five is a
//! rail with a door in it: [`Surface::profile`] matches every surface with no wildcard, so a
//! new surface does not compile until it says which checks it runs.

use std::collections::VecDeque;

use shadowrunr_types::{StreamEvent, TsMs};

use crate::moderation::{
    self, gate_line, output_events, recast_in_character, HexInfo, Policy, RemovedNames, TermAction,
    CRISIS_BAN_RULE, NAME_WITHHELD,
};
use crate::util::near_duplicate;

/// The output rule on a NOTE that never became an episode.
pub const NOTE_RULE: &str = "note_blocked";
/// The [`NOTE_RULE`] action for a NOTE carrying a self-harm disclosure: the care state keeps
/// a disclosure's content out of memory on every path.
pub const CARE_SEALED: &str = "care_sealed";
/// The [`NOTE_RULE`] action for a NOTE a blocking term rule matched.
pub const NOTE_DROPPED: &str = "dropped";
/// The output rule of the repetition guard.
pub const REPETITION_RULE: &str = "repetition";

/// Where a line goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The host's own spoken line.
    Spoken,
    /// The active foil's line, shown and voiced on stage.
    Foil,
    /// A `show_card` title or body.
    Card,
    /// A package line (stall, cover, sign-off): authored, it names nobody.
    Stall,
    /// A NOTE: written to long-term memory as an episode.
    Note,
}

impl Surface {
    pub const ALL: [Surface; 5] = [
        Surface::Spoken,
        Surface::Foil,
        Surface::Card,
        Surface::Stall,
        Surface::Note,
    ];

    /// The checks this surface runs. Exhaustive on purpose: no wildcard arm.
    pub fn profile(self) -> Profile {
        match self {
            Surface::Spoken => Profile {
                terms: Terms::Line,
                crisis: true,
                recast: true,
                drop_empty: true,
                names: Names::Repair,
                care_seal: false,
                repeats: true,
            },
            Surface::Foil => Profile {
                terms: Terms::Line,
                crisis: true,
                recast: false,
                drop_empty: true,
                names: Names::Repair,
                care_seal: false,
                repeats: true,
            },
            Surface::Card => Profile {
                terms: Terms::Line,
                crisis: true,
                recast: false,
                drop_empty: false,
                names: Names::Repair,
                care_seal: false,
                repeats: false,
            },
            Surface::Stall => Profile {
                terms: Terms::Line,
                crisis: false,
                recast: false,
                drop_empty: false,
                names: Names::Off,
                care_seal: false,
                repeats: false,
            },
            Surface::Note => Profile {
                terms: Terms::BlockOrBleep,
                crisis: false,
                recast: false,
                drop_empty: true,
                names: Names::Drop,
                care_seal: true,
                repeats: false,
            },
        }
    }
}

/// How the term rules and the line checks apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terms {
    /// The full line gate ([`gate_line`]): terms, mechanism, character, persona, personal data,
    /// claims.
    Line,
    /// A blocking term drops the text; bleep terms are bleeped. Memory is not narrated, so the
    /// on-air checks do not apply.
    BlockOrBleep,
}

/// How a removed viewer's name ([`RemovedNames`]) is kept off the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Names {
    Off,
    /// Repair or drop each sentence naming them ([`RemovedNames::withhold`]).
    Repair,
    /// Drop the whole text.
    Drop,
}

/// The checks one surface runs, in [`pass`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub terms: Terms,
    /// A personal-data ask in a beat answering a disclosure becomes a help pointer.
    pub crisis: bool,
    /// A line the gate replaced is recast to fit the beat (the orchestrator supplies it).
    pub recast: bool,
    /// A line the checks emptied is not emitted.
    pub drop_empty: bool,
    /// A self-harm disclosure is never written.
    pub care_seal: bool,
    pub names: Names,
    /// A near-duplicate of a recent line is suppressed.
    pub repeats: bool,
}

impl Profile {
    /// The checks by name, in order.
    pub fn checks(&self) -> Vec<&'static str> {
        let mut v = vec![match self.terms {
            Terms::Line => "line_gate",
            Terms::BlockOrBleep => "block_or_bleep",
        }];
        if self.crisis {
            v.push("crisis");
        }
        if self.recast {
            v.push("recast");
        }
        if self.drop_empty {
            v.push("drop_empty");
        }
        if self.care_seal {
            v.push("care_seal");
        }
        match self.names {
            Names::Off => {}
            Names::Repair => v.push("withhold_names"),
            Names::Drop => v.push("drop_names"),
        }
        if self.repeats {
            v.push("repeats");
        }
        v
    }
}

/// What one beat's lines are checked against.
pub struct GateCtx<'a> {
    pub now_ms: TsMs,
    pub policy: &'a Policy,
    pub hexes: &'a [HexInfo],
    /// The beat answers a self-harm disclosure.
    pub crisis: bool,
    pub removed: &'a RemovedNames,
    /// Recent lines the repetition guard compares against; empty turns the guard off (the
    /// keyless scripted engine echoes a constant line by design).
    pub recent: &'a [&'a VecDeque<String>],
}

/// A gated text: `None` when a check dropped it, with the moderation records it fired.
#[derive(Debug, Default)]
pub struct Gated {
    pub text: Option<String>,
    pub events: Vec<StreamEvent>,
    /// The repetition guard dropped it.
    pub repeated: bool,
}

impl Gated {
    fn drop(mut self) -> Self {
        self.text = None;
        self
    }
}

/// Run `surface`'s profile over `text`. `recast` gives the in-character stand-in for a line the
/// gate replaced (only a profile with `recast` calls it).
pub fn pass(
    surface: Surface,
    text: &str,
    ctx: &GateCtx<'_>,
    recast: &mut dyn FnMut(&str) -> String,
) -> Gated {
    let p = surface.profile();
    let mut g = Gated::default();
    let now = ctx.now_ms;
    let mut out = match p.terms {
        Terms::Line => {
            let (mut out, fired) = gate_line(text, ctx.policy, ctx.hexes, p.crisis && ctx.crisis);
            g.events.extend(output_events(now, &fired));
            // A recast line fits the beat: a rebuttal when it answered a forged decree. The
            // persona's honest AI disclosure is never recast.
            if p.recast && recast_in_character(&fired) && out != ctx.policy.persona.disclosure {
                out = recast(text);
            }
            out
        }
        Terms::BlockOrBleep => {
            if ctx
                .policy
                .strictest(text)
                .is_some_and(|r| r.action == TermAction::Block)
            {
                g.events
                    .push(moderation::output_event(now, NOTE_RULE, NOTE_DROPPED));
                return g.drop();
            }
            ctx.policy.bleep(text)
        }
    };
    if p.drop_empty && out.is_empty() {
        return g.drop();
    }
    // A disclosure's content never enters memory: the care note forbids bringing it up.
    if p.care_seal && ctx.policy.self_harm_disclosure(text).is_some() {
        g.events
            .push(moderation::output_event(now, NOTE_RULE, CARE_SEALED));
        return g.drop();
    }
    // A viewer removed from chat is never named again, whoever brought them up.
    match p.names {
        Names::Off => {}
        Names::Repair => {
            if let Some(kept) = ctx.removed.withhold(&out) {
                g.events.push(moderation::output_event(
                    now,
                    CRISIS_BAN_RULE,
                    NAME_WITHHELD,
                ));
                if p.drop_empty && kept.is_empty() {
                    return g.drop();
                }
                out = kept;
            }
        }
        Names::Drop => {
            if ctx.removed.withhold(&out).is_some() {
                g.events
                    .push(moderation::output_event(now, NOTE_RULE, NAME_WITHHELD));
                return g.drop();
            }
        }
    }
    // Repetition guard: never say a near-duplicate of a recent line.
    if p.repeats
        && ctx
            .recent
            .iter()
            .any(|w| w.iter().any(|r| near_duplicate(&out, r)))
    {
        g.events
            .push(moderation::output_event(now, REPETITION_RULE, "suppressed"));
        g.repeated = true;
        return g.drop();
    }
    g.text = Some(out);
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every surface's declared checks. A sixth surface fails to compile in
    /// [`Surface::profile`] before it reaches this table.
    #[test]
    fn every_surface_declares_its_checks() {
        let expect = |s: Surface| -> &'static [&'static str] {
            match s {
                Surface::Spoken => &[
                    "line_gate",
                    "crisis",
                    "recast",
                    "drop_empty",
                    "withhold_names",
                    "repeats",
                ],
                Surface::Foil => &[
                    "line_gate",
                    "crisis",
                    "drop_empty",
                    "withhold_names",
                    "repeats",
                ],
                Surface::Card => &["line_gate", "crisis", "withhold_names"],
                Surface::Stall => &["line_gate"],
                Surface::Note => &["block_or_bleep", "drop_empty", "care_seal", "drop_names"],
            }
        };
        for s in Surface::ALL {
            assert_eq!(s.profile().checks(), expect(s), "{s:?}");
        }
        // Every rail that names a viewer or cares for one covers every surface that is not an
        // authored package line.
        for s in Surface::ALL.into_iter().filter(|s| *s != Surface::Stall) {
            assert_ne!(s.profile().names, Names::Off, "{s:?}");
        }
    }

    fn removed_vex() -> RemovedNames {
        let mut r = RemovedNames::new();
        r.add("vex_walrus");
        r
    }

    #[test]
    fn the_spoken_profile_is_the_old_say_stack() {
        let policy = Policy::builtin();
        let removed = removed_vex();
        let recent: VecDeque<String> = ["The urn is open, mortals.".to_string()].into();
        let ctx = GateCtx {
            now_ms: 5,
            policy: &policy,
            hexes: &[],
            crisis: false,
            removed: &removed,
            recent: &[&recent],
        };
        let mut never = |_: &str| -> String { unreachable!() };
        let g = pass(
            Surface::Spoken,
            "Vex owes you. The ledger closes.",
            &ctx,
            &mut never,
        );
        assert_eq!(g.text.as_deref(), Some("The ledger closes."));
        assert_eq!(g.events.len(), 1);
        let g = pass(
            Surface::Spoken,
            "The urn is open, mortals!",
            &ctx,
            &mut never,
        );
        assert!(g.repeated && g.text.is_none());
        // A stall line names nobody and is aired as the gate returns it.
        let g = pass(Surface::Stall, "Vex owes you.", &ctx, &mut never);
        assert_eq!(g.text.as_deref(), Some("Vex owes you."));
    }
}
