//! Room events the segment runtime reports only in its state text: a vote that closed, a
//! handover that became due. Each is queued once and announced on the next beat that is not
//! a safety beat (see [`super::Orchestrator::plan`]); "still true" is not news, so a result
//! already announced, or already standing when the watch began, is never queued again.
//!
//! `SegmentContext` has no structured field for these, so the watch reads the lines the
//! runtime writes into `state_summary`: `open crowd poll p1: "…" | …`, `last poll p1 closed:
//! …`, `foil vote f1 closed: TIE …`, `PLEBISCITE on h3 decided: …`, `active foil: …` and
//! `HANDOVER DUE: …`. A wording change there silences an announcement; it never repeats one.
//!
//! **Cues** (SEG-03 Table Read): a state's opening job is queued once, like a room event, so
//! it is not lost to the chat a beat answers. In the `intro` state the `TABLE READ with
//! <guest>:` line cues the guest's introduction and the vote; a `FREE TALK after "<title>"`
//! line cues the first talk beat after that scene with its talking points (the scene's
//! `notes`, which the runtime writes into that line). A cue stands even on the watch's first
//! beat (it is a job, not news), takes at most one pick ([`CUE_PICKS`]) so the cue has room,
//! and lapses unannounced once its state is over (sbx-20261024-194036-10m: the intro and the
//! first free talk answered chat only, never the guest, the vote or the scene).

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
    /// A cue: the segment state it belongs to (it lapses once the state is over), and so at
    /// most [`CUE_PICKS`] picks beside it. `None` for a room event.
    pub cue_state: Option<String>,
}

/// The state summary line of a Table Read (`TABLE READ with <guest>: round …`).
pub const TABLE_READ_PREFIX: &str = "TABLE READ with ";
/// The state summary line of the free talk after a scene (`FREE TALK after "<title>" …`).
pub const FREE_TALK_PREFIX: &str = "FREE TALK after \"";
/// The Table Read's opening state, whose beat introduces the guest and the vote.
pub const INTRO_STATE: &str = "intro";
/// The most chat a cue's beat answers beside it.
pub const CUE_PICKS: usize = 1;

/// What the state text said about votes and the handover, beat to beat.
#[derive(Debug, Default, Clone)]
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
    /// The votes open at the last beat: (poll id, question).
    open: Vec<(String, String)>,
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

/// The intro's cue: bring the guest on and explain the vote.
fn intro_cue(guest: &str, state: &str) -> RoomEvent {
    let name = guest.replace(['-', '_'], " ");
    RoomEvent {
        label: "table read intro".into(),
        note: format!(
            "CUE: the Table Read opens now. Bring tonight's guest on: introduce {} by name and \
             who they are (their sheet is under Active foil above), let them say one line, and \
             explain the vote: chat picks each scene with `!vote <title>`, you two perform it, \
             then you talk about it honestly.",
            one_line(&name, 60)
        ),
        connector: None,
        cue_state: Some(state.to_string()),
    }
}

/// The first free talk beat's cue: open it from the scene's talking points.
fn talk_cue(title: &str, line: &str, state: &str) -> RoomEvent {
    RoomEvent {
        label: "free talk opens".into(),
        note: format!(
            "CUE: the scene \"{}\" just ended. Open the free talk from its talking points now: \
             take one, give your honest verdict on the scene (what landed, what died, and why) \
             and put it to chat. {}",
            one_line(title, 120),
            one_line(line, 700)
        ),
        connector: None,
        cue_state: Some(state.to_string()),
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
        let mut cues: Vec<(String, RoomEvent)> = Vec::new();
        self.open.clear();
        for line in summary.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("open ") {
                if let Some((kind, id, q)) = parse_open(rest) {
                    self.questions.insert(id.to_string(), one_line(q, 200));
                    self.open.push((id.to_string(), one_line(q, 80)));
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
                            cue_state: None,
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
                            cue_state: None,
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
                        cue_state: None,
                    },
                ));
            } else if let Some(rest) = line.strip_prefix(TABLE_READ_PREFIX) {
                if state == INTRO_STATE {
                    let guest = rest.split(':').next().unwrap_or(rest).trim();
                    cues.push((format!("table-read-intro {guest}"), intro_cue(guest, state)));
                }
            } else if let Some(rest) = line.strip_prefix(FREE_TALK_PREFIX) {
                let title = rest.split('"').next().unwrap_or(rest);
                cues.push((format!("free-talk {title}"), talk_cue(title, line, state)));
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
                        cue_state: None,
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
        // A cue lapses with its state; a new one stands even on the first beat.
        self.pending
            .retain(|e| e.cue_state.as_deref().is_none_or(|s| s == state));
        for (key, ev) in cues {
            if self.announced.insert(key) {
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
                cue_state: None,
            });
        }
        self.handover_due = due;
    }

    /// The announcements owed, taken once; without `cues`, the cues stay pending.
    pub fn take(&mut self, cues: bool) -> Vec<RoomEvent> {
        let (taken, kept) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|e| cues || e.cue_state.is_none());
        self.pending = kept;
        taken
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The votes open at the last beat: (poll id, question).
    pub fn open_polls(&self) -> &[(String, String)] {
        &self.open
    }
}
