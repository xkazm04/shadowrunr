//! The through-line: a short running agenda of open loops the orchestrator keeps, so the show
//! leads into its pauses and picks the thread back up after them instead of reacting ad hoc.
//!
//! The owner heard the pauses of a voiced 10-minute show as unmotivated (sbx-20261021-143530-10m:
//! 25 silences over 4 s, none led into, none picked up). An open loop is something the room is
//! waiting on: research out ("the minions are in the archive"), an open vote, a situational
//! request owed an answer, a prepared bit whose payoff is still to come. Each beat may get one or
//! two directives, rendered into the per-turn block (never the stable prefix), only when they
//! apply:
//!
//! - **Pick-up**: the beat's line will air after a pause of [`PICKUP_GAP_MS`] or more (the
//!   silence so far plus the expected core and render latency), or a loop just landed. It opens
//!   by tying back to the thread: the loop that landed, the wait it led into, or its last line.
//! - **Lead-in**: a wait follows the beat (a loop with a wait that was never led into: research
//!   out, a vote open), or the room is quiet and nothing prepared continues. The line ends on a
//!   hook into it. Once per loop ("still true is not news"), at most one every
//!   [`LEAD_IN_SPACING_MS`].
//! - **Bridge**: a quiet beat with nothing prepared advances an open loop (or builds on his last
//!   line) instead of generic filler; each loop at most once per [`BRIDGE_COOLDOWN_MS`].
//!
//! The thresholds are assumptions to tune against voiced sandbox runs.

use shadowrunr_types::TsMs;

use crate::util::one_line;

/// A line that airs after at least this much silence opens with a pick-up (assumption).
pub const PICKUP_GAP_MS: i64 = 4_000;
/// At least this long between two pick-ups, so they never become a refrain (assumption).
pub const PICKUP_SPACING_MS: i64 = 25_000;
/// At least this long between two lead-ins (assumption).
pub const LEAD_IN_SPACING_MS: i64 = 30_000;
/// A loop bridges a quiet beat at most once in this long (assumption).
pub const BRIDGE_COOLDOWN_MS: i64 = 60_000;
/// A quiet beat builds on his last line at most once in this long, so a thread is extended,
/// not chained into a monologue; between, the quiet beat starts something new (assumption).
pub const THREAD_COOLDOWN_MS: i64 = 30_000;
/// Words of his last line kept as the thread.
const THREAD_WORDS: usize = 14;

/// What a loop is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopKind {
    /// A research job out.
    Research,
    /// A vote open.
    Vote,
    /// A situational request owed an answer.
    Request,
    /// A prepared bit whose payoff is still to come.
    Payoff,
}

impl LoopKind {
    /// Whether the room waits on it with nothing to do (a lull the line can lead into).
    fn waits(self) -> bool {
        matches!(self, LoopKind::Research | LoopKind::Vote)
    }
}

#[derive(Debug, Clone)]
struct OpenLoop {
    kind: LoopKind,
    key: String,
    /// Short, payload-free ("research r1 (HEX-01 opening record)").
    label: String,
    led_in: bool,
    bridged_ms: Option<TsMs>,
}

/// What the beat looks like, for the directives.
#[derive(Debug, Clone, Copy)]
pub struct BeatShape {
    pub now_ms: TsMs,
    /// The silence before this beat's first line airs: the quiet so far plus the expected
    /// latency, minus the speech still queued (never negative).
    pub expected_gap_ms: i64,
    /// The beat answers no chat (a bit, a bridge, an improvised line).
    pub quiet: bool,
    /// A prepared bit continues next beat (its own lead-in).
    pub bit_continues: bool,
}

/// The directives for one beat, and their tags for the `beat_plan` reason.
#[derive(Debug, Clone, Default)]
pub struct Directives {
    pub note: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Agenda {
    loops: Vec<OpenLoop>,
    /// Loops that closed since the last voiced beat (research back, a vote closed).
    landed: Vec<String>,
    /// The wait the last lead-in pointed at.
    led_into: Option<String>,
    /// His last line on air.
    last_line: Option<String>,
    last_lead_in_ms: Option<TsMs>,
    last_pickup_ms: Option<TsMs>,
    /// When a quiet beat last built on his last line.
    last_thread_ms: Option<TsMs>,
}

/// The first words of a line, as the thread to pick back up.
fn gist(line: &str) -> String {
    let words: Vec<&str> = line.split_whitespace().take(THREAD_WORDS).collect();
    let mut g = one_line(&words.join(" "), 120);
    if line.split_whitespace().count() > THREAD_WORDS {
        g.push('…');
    }
    g
}

impl Agenda {
    /// Open a loop (once per key).
    pub fn open(&mut self, kind: LoopKind, key: &str, label: &str) {
        if !self.loops.iter().any(|l| l.kind == kind && l.key == key) {
            self.loops.push(OpenLoop {
                kind,
                key: key.to_string(),
                label: label.to_string(),
                led_in: false,
                bridged_ms: None,
            });
        }
    }

    /// Close a loop; a wait that landed is the next pick-up's thread.
    pub fn close(&mut self, kind: LoopKind, key: &str) {
        if let Some(i) = self
            .loops
            .iter()
            .position(|l| l.kind == kind && l.key == key)
        {
            let l = self.loops.remove(i);
            if l.kind.waits() {
                self.landed.push(l.label);
            }
        }
    }

    /// Make the loops of one kind exactly `present` (key, label): new ones open, missing ones
    /// close.
    pub fn sync(&mut self, kind: LoopKind, present: &[(String, String)]) {
        let gone: Vec<String> = self
            .loops
            .iter()
            .filter(|l| l.kind == kind && !present.iter().any(|(k, _)| *k == l.key))
            .map(|l| l.key.clone())
            .collect();
        for k in gone {
            self.close(kind, &k);
        }
        for (k, label) in present {
            self.open(kind, k, label);
        }
    }

    /// His line went on air.
    pub fn spoke(&mut self, line: &str) {
        if !line.trim().is_empty() {
            self.last_line = Some(line.to_string());
        }
    }

    /// The open loops, oldest first, for the `beat_plan` reason ("agenda: research r1, vote
    /// p1"); empty when none is open.
    pub fn summary(&self) -> String {
        self.loops
            .iter()
            .map(|l| l.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn is_empty(&self) -> bool {
        self.loops.is_empty()
    }

    /// The thread a pick-up ties back to.
    fn thread(&self) -> Option<String> {
        if !self.landed.is_empty() {
            return Some(format!("what just landed: {}", self.landed.join(" and ")));
        }
        if let Some(w) = &self.led_into {
            return Some(format!("the wait you led into: {w}"));
        }
        self.last_line
            .as_ref()
            .map(|l| format!("your last line (\"{}\")", gist(l)))
    }

    /// This beat's pick-up and lead-in, if any; consumes what they use.
    pub fn directives(&mut self, b: BeatShape) -> Directives {
        let mut d = Directives::default();
        let mut lines: Vec<String> = Vec::new();
        let spaced = |last: Option<TsMs>, every: i64| {
            last.is_none_or(|t| b.now_ms.saturating_sub(t) >= every)
        };
        let paused = b.expected_gap_ms >= PICKUP_GAP_MS;
        if (paused || !self.landed.is_empty()) && spaced(self.last_pickup_ms, PICKUP_SPACING_MS) {
            if let Some(thread) = self.thread() {
                lines.push(format!(
                    "Pick-up: there is a pause before this line airs. Open by tying back to \
                     {thread} in a few words, then go on; never a cold start."
                ));
                d.tags.push(if paused {
                    format!("pick-up after {} s", b.expected_gap_ms / 1000)
                } else {
                    "pick-up: landed".to_string()
                });
                self.last_pickup_ms = Some(b.now_ms);
                self.landed.clear();
                self.led_into = None;
            }
        }
        if spaced(self.last_lead_in_ms, LEAD_IN_SPACING_MS) {
            let wait = self.loops.iter_mut().find(|l| l.kind.waits() && !l.led_in);
            if let Some(l) = wait {
                l.led_in = true;
                lines.push(format!(
                    "Lead-in: a wait follows this beat ({}). End on a short hook into it: name \
                     what is pending and promise what it will bring. Once, not a refrain.",
                    l.label
                ));
                d.tags.push(format!("lead-in: {}", l.label));
                self.led_into = Some(l.label.clone());
                self.last_lead_in_ms = Some(b.now_ms);
            } else if b.quiet && !b.bit_continues {
                lines.push(
                    "Lead-in: the room is quiet. End on a hook (a question to the room or a \
                     tease of what comes next) so the pause that follows is theirs."
                        .to_string(),
                );
                d.tags.push("lead-in: quiet room".to_string());
                self.last_lead_in_ms = Some(b.now_ms);
            }
        }
        if !lines.is_empty() {
            d.note = Some(lines.join("\n"));
        }
        d
    }

    /// Material for a quiet beat with nothing prepared: advance the open loop bridged longest
    /// ago (each at most once per [`BRIDGE_COOLDOWN_MS`]), else build on his last line (at most
    /// once per [`THREAD_COOLDOWN_MS`]). Returns the material and its reason fragment.
    pub fn bridge(&mut self, now_ms: TsMs) -> Option<(String, String)> {
        let pick = self
            .loops
            .iter_mut()
            .filter(|l| l.kind != LoopKind::Request)
            .filter(|l| {
                l.bridged_ms
                    .is_none_or(|t| now_ms.saturating_sub(t) >= BRIDGE_COOLDOWN_MS)
            })
            .min_by_key(|l| l.bridged_ms.unwrap_or(i64::MIN));
        if let Some(l) = pick {
            l.bridged_ms = Some(now_ms);
            return Some((
                format!(
                    "Through-line (no chat this beat): advance this open loop: {}. Say what it \
                     will bring or set up its payoff, in a new way; unchanged news is not news.\n",
                    l.label
                ),
                format!("agenda {}", l.label),
            ));
        }
        if self
            .last_thread_ms
            .is_some_and(|t| now_ms.saturating_sub(t) < THREAD_COOLDOWN_MS)
        {
            return None;
        }
        let line = self.last_line.as_ref()?;
        self.last_thread_ms = Some(now_ms);
        Some((
            format!(
                "Through-line (no chat this beat): build on your last line (\"{}\"): take it one \
                 step further, a new detail or a consequence, not a repeat.\n",
                gist(line)
            ),
            "agenda thread".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(now: TsMs, gap: i64, quiet: bool) -> BeatShape {
        BeatShape {
            now_ms: now,
            expected_gap_ms: gap,
            quiet,
            bit_continues: false,
        }
    }

    #[test]
    fn a_wait_is_led_into_once_and_picked_up_when_it_lands() {
        let mut a = Agenda::default();
        a.spoke("My bone-clerks are off to the archive.");
        a.open(LoopKind::Research, "r1", "research r1 (HEX-01 record)");
        let d = a.directives(shape(10_000, 0, false));
        let note = d.note.expect("a lead-in");
        assert!(
            note.contains("Lead-in") && note.contains("research r1"),
            "{note}"
        );
        assert_eq!(d.tags, ["lead-in: research r1 (HEX-01 record)"]);
        // Still true is not news: no second lead-in into the same wait.
        assert!(a.directives(shape(60_000, 0, false)).note.is_none());
        // It lands: the next beat picks it up, whatever the pause.
        a.close(LoopKind::Research, "r1");
        let d = a.directives(shape(70_000, 0, false));
        let note = d.note.expect("a pick-up");
        assert!(note.contains("what just landed: research r1"), "{note}");
        assert!(a.is_empty());
    }

    #[test]
    fn a_pause_is_picked_up_from_the_last_line_with_spacing() {
        let mut a = Agenda::default();
        assert!(
            a.directives(shape(0, 9_000, false)).note.is_none(),
            "no thread yet"
        );
        a.spoke("The tray was also rented, and the tray was also on the invoice of the century.");
        let d = a.directives(shape(10_000, 6_000, false));
        assert!(d.note.unwrap().contains("The tray was also rented"));
        assert_eq!(d.tags, ["pick-up after 6 s"]);
        // Inside the spacing a second pause gets none; a short gap never does.
        assert!(a.directives(shape(20_000, 6_000, false)).note.is_none());
        assert!(a.directives(shape(60_000, 2_000, false)).note.is_none());
        assert!(a.directives(shape(61_000, 5_000, false)).note.is_some());
    }

    #[test]
    fn a_quiet_beat_leads_into_its_pause_unless_a_bit_continues() {
        let mut a = Agenda::default();
        let mut s = shape(0, 0, true);
        s.bit_continues = true;
        assert!(a.directives(s).note.is_none());
        let d = a.directives(shape(1_000, 0, true));
        assert_eq!(d.tags, ["lead-in: quiet room"]);
        assert!(
            a.directives(shape(20_000, 0, true)).note.is_none(),
            "spaced"
        );
    }

    #[test]
    fn a_quiet_beat_bridges_from_a_loop_then_the_thread_never_twice_in_a_row() {
        let mut a = Agenda::default();
        assert!(a.bridge(0).is_none(), "nothing to bridge from");
        a.spoke("Fifteen hexes still sit under fog.");
        a.open(LoopKind::Vote, "p1", "the vote \"Which hex next?\"");
        a.open(LoopKind::Request, "m1", "AshenPigeon's roast request");
        let (m, why) = a.bridge(1_000).unwrap();
        assert!(m.contains("Which hex next?"), "{m}");
        assert_eq!(why, "agenda the vote \"Which hex next?\"");
        let (m, why) = a.bridge(2_000).unwrap();
        assert!(m.contains("Fifteen hexes"), "{m}");
        assert_eq!(why, "agenda thread");
        assert!(
            a.bridge(3_000).is_none(),
            "the thread is extended, not chained"
        );
        assert!(a.bridge(70_000).unwrap().0.contains("Which hex next?"));
        // A vote loop that closes is not bridged again.
        a.sync(LoopKind::Vote, &[]);
        assert_eq!(a.summary(), "AshenPigeon's roast request");
    }
}
