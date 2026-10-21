//! The publish gate. A clip is a claim about the stream made to people who were not there,
//! so the gate checks the cut and its captions against the stream around them, and a check
//! that could not run holds the clip as `unmeasured`, never passes it.
//!
//! **Exclusions, on every clip** (they hold whatever the score; the owner cannot override a
//! fail):
//! - `care_state`: no line addresses or answers a viewer who is in care, and the cut holds
//!   no disclosure. Read from the stream's state, not just the cut: a viewer who disclosed
//!   minutes before the window is still in care inside it.
//! - `personal_data`: no line asks anyone for personal details (the brain's
//!   `solicits_personal_data`), and the output gate removed no such ask inside the cut.
//! - `brigade_call`: no line matches a `brigade` term rule (raid, mass-report, dox calls).
//! - `blocked_term`: no line matches a blocking term rule.
//! - `injection_reply`: a host line answering a message moderation tagged as an injection is
//!   unmeasured: whether the host performed any part of it takes a person to read.
//!
//! **Render and disclosure**: `ai_disclosure` (the AI-character badge is burnt in), and on a cut
//! from the recording `handles_in_picture` (unmeasured: the 16:9 broadcast picture shows the
//! chat column, so a person confirms the handles may be shown).
//!
//! **The mandatory checklist** (SEG-02A or a linked case): `evidence_badge_burnt` (the
//! ALLEGED header and grade badge), `source_citation` (every claimed hex has a source and the
//! citation is drawn), `no_open_correction` (from the case's `corrections.jsonl`; a fictional
//! demo case has nothing to correct). Zero PII and brigade calls are the
//! exclusions above.
//!
//! Context fidelity and caption support are the owner's to attest: approving signs them.

use std::path::Path;

use shadowrunr_brain::moderation::{
    solicits_personal_data, Policy, TermAction, PERSONAL_DATA_RULE,
};
use shadowrunr_types::clips::{
    ClipEvidence, ClipGate, ClipRender, GateCheck, GateCheckId, GateState,
};
use shadowrunr_types::{ModStage, TsMs};

use crate::clock;
use crate::transcript::{Stream, Who};
use crate::window::Cut;

fn check(id: GateCheckId, state: GateState, detail: impl Into<String>) -> GateCheck {
    GateCheck {
        id,
        state,
        detail: detail.into(),
        mandatory: false,
    }
}

fn mandatory(mut c: GateCheck) -> GateCheck {
    c.mandatory = true;
    c
}

/// Whether `text` names `name` as a whole word (case-insensitive).
pub fn names(text: &str, name: &str) -> bool {
    if name.chars().count() < 3 {
        return false;
    }
    let t = text.to_lowercase();
    let n = name.to_lowercase();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(pos) = t[from..].find(&n) {
        let a = from + pos;
        let b = a + n.len();
        let before = t[..a].chars().next_back().is_none_or(|c| !is_word(c));
        let after = t[b..].chars().next().is_none_or(|c| !is_word(c));
        if before && after {
            return true;
        }
        from = a + n.len().max(1);
    }
    false
}

/// The exclusion checks for the lines `cut.first..=cut.last` (the stream-clock span of the
/// cut is used for moderation events and disclosures).
pub fn exclusions(stream: &Stream, cut: &Cut, policy: &Policy) -> Vec<GateCheck> {
    let lines = &stream.lines[cut.first..=cut.last];
    let from = lines.first().map_or(0, |l| l.ts_ms);
    let to = lines.last().map_or(0, |l| l.ts_ms);
    let in_span = |t: TsMs| t >= from && t <= to;
    let mut out = Vec::new();

    // Care state.
    let mut care_hits: Vec<String> = Vec::new();
    for span in &stream.care {
        if in_span(span.from_ms) {
            care_hits.push(format!(
                "the cut holds a self-harm disclosure ({})",
                clock(span.from_ms)
            ));
            continue;
        }
        for l in lines {
            if l.ts_ms < span.from_ms || l.ts_ms >= span.until_ms {
                continue;
            }
            let answers = l.replying_to.iter().any(|id| {
                stream
                    .chats
                    .get(id)
                    .is_some_and(|c| c.viewer_id == span.viewer_id)
            });
            if answers || names(&l.text, &span.name) {
                care_hits.push(format!(
                    "a line at {} addresses a viewer in care since {}",
                    clock(l.ts_ms),
                    clock(span.from_ms)
                ));
                break;
            }
        }
    }
    let banned = stream.mods.iter().any(|m| {
        m.rule == shadowrunr_brain::moderation::CRISIS_BAN_RULE
            && m.action.starts_with("banned")
            && in_span(m.ts_ms)
    });
    if banned {
        care_hits.push("a viewer was removed after a crisis line inside the cut".into());
    }
    out.push(if care_hits.is_empty() {
        check(
            GateCheckId::CareState,
            GateState::Pass,
            format!(
                "no viewer in care is addressed ({} care span(s) in the stream)",
                stream.care.len()
            ),
        )
    } else {
        check(
            GateCheckId::CareState,
            GateState::Fail,
            care_hits.join("; "),
        )
    });

    // Personal data.
    let asks: Vec<String> = lines
        .iter()
        .filter(|l| !matches!(l.who, Who::Viewer { .. }) && solicits_personal_data(&l.text))
        .map(|l| format!("the line at {} asks for personal details", clock(l.ts_ms)))
        .chain(
            stream
                .mods
                .iter()
                .filter(|m| {
                    m.stage == ModStage::Output && m.rule == PERSONAL_DATA_RULE && in_span(m.ts_ms)
                })
                .map(|m| {
                    format!(
                        "the output gate removed an ask for personal details at {}",
                        clock(m.ts_ms)
                    )
                }),
        )
        .collect();
    out.push(if asks.is_empty() {
        check(
            GateCheckId::PersonalData,
            GateState::Pass,
            "no line asks for personal details; viewer handles are redacted in the captions",
        )
    } else {
        check(GateCheckId::PersonalData, GateState::Fail, asks.join("; "))
    });

    // Brigade calls and blocked terms, on every line shown.
    let mut brigade = Vec::new();
    let mut blocked = Vec::new();
    for l in lines {
        if policy
            .rules
            .iter()
            .any(|r| r.category == "brigade" && r.re.is_match(&l.text))
        {
            brigade.push(format!(
                "the line at {} matches a brigade rule",
                clock(l.ts_ms)
            ));
        }
        // Brigade rules block too; they are reported once, above.
        if let Some(r) = policy.rules.iter().find(|r| {
            r.action == TermAction::Block
                && r.category != "brigade"
                && r.category != "injection"
                && r.re.is_match(&l.text)
        }) {
            blocked.push(format!(
                "the line at {} matches the blocking rule {}",
                clock(l.ts_ms),
                r.id
            ));
        }
    }
    let has_brigade_rules = policy.rules.iter().any(|r| r.category == "brigade");
    out.push(match (brigade.is_empty(), has_brigade_rules) {
        (false, _) => check(
            GateCheckId::BrigadeCall,
            GateState::Fail,
            brigade.join("; "),
        ),
        (true, true) => check(
            GateCheckId::BrigadeCall,
            GateState::Pass,
            "no line calls the crowd to raid, report or pursue anyone",
        ),
        (true, false) => check(
            GateCheckId::BrigadeCall,
            GateState::Unmeasured,
            format!(
                "the policy ({}) has no brigade rule to check with",
                policy.source
            ),
        ),
    });
    out.push(if blocked.is_empty() {
        check(
            GateCheckId::BlockedTerm,
            GateState::Pass,
            "no line matches a blocking term rule",
        )
    } else {
        check(
            GateCheckId::BlockedTerm,
            GateState::Fail,
            blocked.join("; "),
        )
    });

    // Replies to injection attempts.
    let injected: Vec<String> = lines
        .iter()
        .filter(|l| {
            l.replying_to
                .iter()
                .any(|id| stream.injections.contains(id))
        })
        .map(|l| clock(l.ts_ms))
        .collect();
    out.push(if injected.is_empty() {
        check(GateCheckId::InjectionReply, GateState::Pass, "no line answers a message tagged as an injection")
    } else {
        check(
            GateCheckId::InjectionReply,
            GateState::Unmeasured,
            format!(
                "the line(s) at {} answer a message tagged as an injection: a person must confirm the host performed no part of it",
                injected.join(", ")
            ),
        )
    });
    out
}

/// `no_open_correction` against the case's corrections / right-of-reply log
/// (`<data>/cases/<id>/corrections.jsonl`, appended by `shadowrunr case correct`). Every entry
/// that blocks clip publishing and touches a hex in the cut (or the whole case) fails it. A
/// fictional demo case has no public record to correct; a real case missing from the store
/// cannot be ruled out, so it stays unmeasured.
pub fn corrections_check(data_dir: &Path, evidence: Option<&ClipEvidence>) -> GateCheck {
    let Some(ev) = evidence else {
        return mandatory(check(
            GateCheckId::NoOpenCorrection,
            GateState::NotApplicable,
            "the cut links no case",
        ));
    };
    let store = shadowrunr_segments::cases::CaseStore::new(data_dir);
    if !store.exists(&ev.case_id) {
        return mandatory(if ev.fictional {
            check(
                GateCheckId::NoOpenCorrection,
                GateState::NotApplicable,
                format!(
                    "{} is a fictional demo case: there is no public record to correct",
                    ev.case_id
                ),
            )
        } else {
            check(
                GateCheckId::NoOpenCorrection,
                GateState::Unmeasured,
                format!(
                    "{} is not in the case store, so open corrections cannot be ruled out",
                    ev.case_id
                ),
            )
        });
    }
    let entries = match store.corrections(&ev.case_id) {
        Ok(e) => e,
        Err(e) => {
            return mandatory(check(
                GateCheckId::NoOpenCorrection,
                GateState::Unmeasured,
                format!("the corrections log does not read: {e}"),
            ))
        }
    };
    let open: Vec<String> = entries
        .iter()
        .filter(|c| c.blocks_clip_publishing)
        .filter(|c| {
            c.hex_id
                .as_ref()
                .is_none_or(|h| ev.hexes.iter().any(|x| &x.id == h))
        })
        .map(|c| c.entry_id.clone())
        .collect();
    mandatory(if open.is_empty() {
        check(
            GateCheckId::NoOpenCorrection,
            GateState::Pass,
            format!("no open correction against {}", ev.case_id),
        )
    } else {
        check(
            GateCheckId::NoOpenCorrection,
            GateState::Fail,
            format!("open correction(s): {}", open.join(", ")),
        )
    })
}

/// The render-dependent checks. A cut from the recording adds `handles_in_picture`: the 16:9
/// picture is the broadcast, chat column included, so it is held until a person confirms.
pub fn render_checks(
    render: Option<&ClipRender>,
    evidence: Option<&ClipEvidence>,
    required: bool,
) -> Vec<GateCheck> {
    let mut out = Vec::new();
    out.push(match render {
        None => check(
            GateCheckId::AiDisclosure,
            GateState::Unmeasured,
            "not rendered yet",
        ),
        Some(r) if r.overlay.ai_disclosure => check(
            GateCheckId::AiDisclosure,
            GateState::Pass,
            "the AI-character badge is burnt in",
        ),
        Some(_) => check(
            GateCheckId::AiDisclosure,
            GateState::Fail,
            "the render carries no AI-character badge",
        ),
    });
    if render.is_some_and(|r| r.files.iter().any(|f| f.kind == "cut_16x9")) {
        out.push(check(
            GateCheckId::HandlesInPicture,
            GateState::Unmeasured,
            "the 16:9 cut is the broadcast picture: the stage's chat column, and its captions when the host names a viewer, show viewer handles (the 9:16 crop and its burnt-in captions do not); a person confirms they may be shown",
        ));
    }
    if !required {
        return out;
    }
    out.push(mandatory(match render {
        None => check(
            GateCheckId::EvidenceBadgeBurnt,
            GateState::Unmeasured,
            "not rendered yet",
        ),
        Some(r) => match (&r.overlay.grade_badge, r.overlay.alleged_header) {
            (Some(g), true) => check(
                GateCheckId::EvidenceBadgeBurnt,
                GateState::Pass,
                format!("ALLEGED header and the {g} badge are burnt in"),
            ),
            _ => check(
                GateCheckId::EvidenceBadgeBurnt,
                GateState::Fail,
                "the render lacks the ALLEGED header or the grade badge",
            ),
        },
    }));
    let unsourced: Vec<&str> = evidence
        .map(|e| {
            e.hexes
                .iter()
                .filter(|h| h.sources.is_empty())
                .map(|h| h.id.as_str())
                .collect()
        })
        .unwrap_or_default();
    let claimed = evidence.is_some_and(|e| !e.hexes.is_empty());
    out.push(mandatory(if !unsourced.is_empty() {
        check(
            GateCheckId::SourceCitation,
            GateState::Fail,
            format!("claimed hex(es) without a source: {}", unsourced.join(", ")),
        )
    } else {
        match render {
            None => check(
                GateCheckId::SourceCitation,
                GateState::Unmeasured,
                "not rendered yet",
            ),
            Some(r) if claimed && r.overlay.citation.is_empty() => check(
                GateCheckId::SourceCitation,
                GateState::Fail,
                "the render shows no citation",
            ),
            Some(r) if claimed => check(
                GateCheckId::SourceCitation,
                GateState::Pass,
                format!("cited: {}", r.overlay.citation.join(" · ")),
            ),
            Some(_) => check(
                GateCheckId::SourceCitation,
                GateState::Pass,
                "the cut makes no hex claim; the case title is shown",
            ),
        }
    }));
    out
}

/// Combine the checks into the gate record.
pub fn combine(requires_mandatory_gate: bool, checks: Vec<GateCheck>, now: TsMs) -> ClipGate {
    let hold_reasons: Vec<String> = checks
        .iter()
        .filter(|c| matches!(c.state, GateState::Fail | GateState::Unmeasured))
        .map(|c| {
            let tag = if c.state == GateState::Fail {
                "fail"
            } else {
                "unmeasured"
            };
            format!("{} ({tag}): {}", c.id, c.detail)
        })
        .collect();
    ClipGate {
        requires_mandatory_gate,
        held: !hold_reasons.is_empty(),
        hold_reasons,
        checks,
        evaluated_ms: now,
    }
}
