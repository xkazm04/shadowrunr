//! Room events the segment runtime reports only in its state text: a vote that closed, a
//! handover that became due. Each is queued once and announced on the next beat that is not
//! a safety beat (see [`super::Orchestrator::plan`]); "still true" is not news, so a result
//! already announced, or already standing when the watch began, is never queued again.
//!
//! `SegmentContext` has no structured field for these, so the watch reads the lines the
//! runtime writes into `state_summary`: `open crowd poll p1: "…" | …`, `last poll p1 closed:
//! …`, `foil vote f1 closed: TIE …`, `PLEBISCITE on h3 decided: …`, `active foil: …` and
//! `HANDOVER DUE: …`. A wording change there silences an announcement; it never repeats one.

use std::collections::{HashMap, HashSet};

use shadowrunr_types::prep::ConnectorKind;

use crate::util::one_line;

/// One announcement owed to the room.
#[derive(Debug, Clone)]
pub struct RoomEvent {
    /// For the `beat_plan` reason ("poll p1 closed (tie)").
    pub label: String,
    /// The per-turn line that asks for the announcement.
    pub note: String,
    /// The prepared connector that fits it, if any.
    pub connector: Option<ConnectorKind>,
}

/// What the state text said about votes and the handover, beat to beat.
#[derive(Debug, Default)]
pub struct RoomWatch {
    started: bool,
    /// Questions of the polls seen open, by poll id.
    questions: HashMap<String, String>,
    /// The foil vote open at the last beat.
    open_foil_vote: Option<String>,
    /// Result lines (and foil wins) already queued or standing at the start.
    announced: HashSet<String>,
    handover_due: bool,
    pending: Vec<RoomEvent>,
}

/// `crowd poll p1: "Question?" | …` into (kind, id, question).
fn parse_open(rest: &str) -> Option<(&str, &str, &str)> {
    let (head, tail) = rest.split_once(": \"")?;
    let (kind, id) = head.rsplit_once(' ')?;
    let question = tail.rsplit_once("\" | ").map_or(tail, |(q, _)| q);
    Some((kind, id, question))
}

fn is_tie(result: &str) -> bool {
    result.contains("TIE")
}

/// What the host must do with a vote's result.
fn act(result: &str) -> &'static str {
    if is_tie(result) {
        "Announce it now as a tie (never call either option the winner), then break the tie \
         yourself and say you are breaking it."
    } else if result.contains("no votes") || result.contains("drew no votes") {
        "Say so now in one line and make the call yourself."
    } else {
        "Announce the result now and act on it."
    }
}

impl RoomWatch {
    /// Read one beat's state; queue what newly happened.
    pub fn observe(&mut self, state: &str, summary: &str) {
        let first = !self.started;
        self.started = true;
        let mut open_foil: Option<String> = None;
        let mut active_foil: Option<&str> = None;
        let mut handover_line: Option<&str> = None;
        let mut found: Vec<(String, RoomEvent)> = Vec::new();
        for line in summary.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("open ") {
                if let Some((kind, id, q)) = parse_open(rest) {
                    self.questions.insert(id.to_string(), one_line(q, 200));
                    if kind == "foil vote" {
                        open_foil = Some(id.to_string());
                    }
                }
            } else if let Some(rest) = line.strip_prefix("last poll ") {
                if let Some((id, result)) = rest.split_once(" closed: ") {
                    let what = match self.questions.get(id) {
                        Some(q) => format!("the vote on \"{q}\" ({id})"),
                        None => format!("the vote {id}"),
                    };
                    found.push((
                        line.to_string(),
                        RoomEvent {
                            label: format!(
                                "poll {id} closed{}",
                                if is_tie(result) { " (tie)" } else { "" }
                            ),
                            note: format!(
                                "ROOM EVENT: {what} just closed: {}. {}",
                                one_line(result, 300),
                                act(result)
                            ),
                            connector: None,
                        },
                    ));
                }
            } else if let Some(rest) = line.strip_prefix("foil vote ") {
                if let Some((id, result)) = rest.split_once(" closed: ") {
                    found.push((
                        line.to_string(),
                        RoomEvent {
                            label: format!("foil vote {id} closed (tie)"),
                            note: format!(
                                "ROOM EVENT: the foil vote {id} just closed: {}. Announce it \
                                 now as a tie, then break it yourself and say you are \
                                 breaking it.",
                                one_line(result, 300)
                            ),
                            connector: None,
                        },
                    ));
                }
            } else if line.starts_with("PLEBISCITE on ")
                && (line.contains(" decided: ")
                    || line.contains(" closed: ")
                    || line.contains(" drew no votes"))
            {
                found.push((
                    line.to_string(),
                    RoomEvent {
                        label: format!(
                            "plebiscite closed{}",
                            if is_tie(line) { " (tie)" } else { "" }
                        ),
                        note: format!(
                            "ROOM EVENT: the Plebiscite just closed: {}. {}",
                            one_line(line, 300),
                            act(line)
                        ),
                        connector: None,
                    },
                ));
            } else if let Some(f) = line.strip_prefix("active foil: ") {
                active_foil = Some(f);
            } else if line.starts_with("HANDOVER DUE") {
                handover_line = Some(line);
            }
        }
        // A foil vote that closed with a winner leaves only the summoned foil behind.
        if let (Some(id), None, Some(foil)) = (&self.open_foil_vote, &open_foil, active_foil) {
            let tie = found
                .iter()
                .any(|(l, _)| l.starts_with(&format!("foil vote {id} ")));
            if !tie {
                let what = match self.questions.get(id) {
                    Some(q) => format!("the foil vote \"{q}\" ({id})"),
                    None => format!("the foil vote {id}"),
                };
                found.push((
                    format!("foil-winner {id}"),
                    RoomEvent {
                        label: format!("foil vote {id} closed"),
                        note: format!(
                            "ROOM EVENT: {what} just closed: {} won and is summoned. Announce \
                             the result now and bring them in.",
                            one_line(foil, 80)
                        ),
                        connector: None,
                    },
                ));
            }
        }
        self.open_foil_vote = open_foil;
        for (key, ev) in found {
            if self.announced.insert(key) && !first {
                self.pending.push(ev);
            }
        }
        let due = state.contains("handover") || handover_line.is_some();
        if due && !self.handover_due {
            let what = handover_line
                .map_or_else(|| "the handover is due".to_string(), |l| one_line(l, 300));
            self.pending.push(RoomEvent {
                label: "handover due".into(),
                note: format!("ROOM EVENT: {what}. Bridge to it now, in your own voice."),
                connector: Some(ConnectorKind::IntoHandover),
            });
        }
        self.handover_due = due;
    }

    /// The announcements owed, taken once.
    pub fn take(&mut self) -> Vec<RoomEvent> {
        std::mem::take(&mut self.pending)
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}
