//! Scene rounds (SEG-03 Table Read, contract appendix "SEG-03 Table Read"): a segment whose
//! table has the [`SCENE_STATE`] plays the stream's [`TableRead`] in rounds. A round is a
//! ballot (while two or more scenes are left), the scene, then free talk; the last scene plays
//! without a ballot. The runtime moves between the round states itself, a clock rule like the
//! cold open's, so a table that plays scenes names the five states and the rules below run
//! them; no code names a segment.
//!
//! - [`INTRO_STATE`]: the host brings the guest on. After `intro_s` the first round starts.
//! - [`BALLOT_STATE`]: the runtime's own poll over the scenes left (their titles, in ballot
//!   order), emitted as an ordinary `start_poll` / `vote` / closed `vote`. A tie is never
//!   settled silently: the first tied scene in ballot order plays, and the state summary tells
//!   the host it is his tie to break, naming that scene so his words match.
//! - [`SCENE_STATE`]: the brain performs the scene up (the state summary's [`SCENE_UP_PREFIX`]
//!   line) and reports its `scene_end`; [`crate::SegmentRuntime::scene_ended`] moves on. A scene
//!   nobody performs ends itself after `scene_max_s`.
//! - [`TALK_STATE`]: free talk about the scene for `talk_s`; the last one lasts until the floor.
//! - [`CLOSING_STATE`]: the host wraps up and signs off.
//!
//! The floor (`floor_s`) keeps the segment from being abandoned early: `switch_segment` is
//! refused before it (`segment_floor`), and so is `sign_off`, unless the host says the stream
//! itself is at its planned end ([`crate::SegmentRuntime::set_stream_ending`], the goodbye
//! window): a stream scheduled to end before the floor still signs off on time
//! (sbx-20261024-194036-10m: SEG-03 began at 04:03 of a 10-minute show, and three sign-offs in
//! the goodbye were refused). At the ceiling the round in progress ends after its scene and
//! the segment closes.
//!
//! The timings are ASSUMPTIONS, uncalibrated, and the config may override them
//! ([`RoundTimes`]). Free talk is 5 minutes, the middle of the format's 4-6: three rounds of a
//! 1-minute ballot, a 2-4 minute scene and 5 minutes of talk, after a 90-second intro, come to
//! about 26-32 minutes, inside the 20-minute floor and the 40-minute ceiling with room for a
//! slow scene.

use shadowrunr_types::scene::Scene;
use shadowrunr_types::TsMs;

use crate::runtime::PollOutcome;
use crate::scenes::TableRead;

/// The host brings the guest on.
pub const INTRO_STATE: &str = "intro";
/// The audience votes on the next scene.
pub const BALLOT_STATE: &str = "ballot";
/// A scene is being performed.
pub const SCENE_STATE: &str = "scene";
/// Free talk after a scene.
pub const TALK_STATE: &str = "talk";
/// The wrap-up and the sign-off.
pub const CLOSING_STATE: &str = "closing";
/// Every state a table that plays scenes must have.
pub const ROUND_STATES: [&str; 5] = [
    INTRO_STATE,
    BALLOT_STATE,
    SCENE_STATE,
    TALK_STATE,
    CLOSING_STATE,
];

/// The state summary line that names the scene to perform: `SCENE UP: <id> "<title>" ...`.
/// The brain reads the id after it (`shadowrunr_brain::scene`).
pub const SCENE_UP_PREFIX: &str = "SCENE UP: ";

/// The output record of a scene the runtime ended itself (nobody performed it).
pub const SCENE_TIMEOUT_RULE: &str = "scene_timeout";

/// The round clock. Every value is an ASSUMPTION (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundTimes {
    /// The intro before the first round.
    pub intro_s: u32,
    /// How long a scene ballot stays open.
    pub ballot_secs: u32,
    /// Free talk after each scene.
    pub talk_s: u32,
    /// `switch_segment` is refused before this, and `sign_off` too unless the stream is
    /// ending; the last talk lasts until it.
    pub floor_s: u32,
    /// From here no new round starts: the segment closes after the scene in progress.
    pub ceiling_s: u32,
    /// A scene still up this long after its cue is ended by the runtime.
    pub scene_max_s: u32,
}

impl RoundTimes {
    pub const DEFAULT: RoundTimes = RoundTimes {
        intro_s: 90,
        ballot_secs: 60,
        talk_s: 300,
        floor_s: 1_200,
        ceiling_s: 2_400,
        scene_max_s: 600,
    };

    /// Consistent numbers: a positive ballot, talk and scene limit, and a ceiling at or above
    /// the floor.
    pub fn check(&self) -> Result<(), String> {
        if self.ballot_secs == 0
            || self.talk_s == 0
            || self.scene_max_s == 0
            || self.ceiling_s < self.floor_s
        {
            return Err(
                "rounds: a positive ballot, talk and scene limit, and a ceiling at or above the \
                 floor"
                    .into(),
            );
        }
        Ok(())
    }
}

impl Default for RoundTimes {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How the scene up was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cue {
    /// The last scene left: no ballot.
    Unvoted,
    /// A ballot chose it (or tied, or drew no votes: the first in ballot order then).
    Ballot {
        poll_id: String,
        outcome: PollOutcome,
    },
}

/// The rounds of one Table Read.
#[derive(Debug, Clone)]
pub(crate) struct Rounds {
    pub guest: String,
    pub scenes: Vec<Scene>,
    /// Indices into `scenes` not played yet, in ballot order.
    pub left: Vec<usize>,
    /// The scene up (cued or being performed), when it was cued and how it was chosen.
    pub up: Option<(usize, TsMs, Cue)>,
    /// Played scenes, in order.
    pub played: Vec<usize>,
    /// When the current free talk began.
    pub talk_since: Option<TsMs>,
    /// Scene ballots opened.
    pub ballots: u32,
    /// The last ballot's id, outcome and the scene it put up.
    pub last_ballot: Option<(String, PollOutcome, usize)>,
    pub times: RoundTimes,
}

impl Rounds {
    pub fn new(tr: &TableRead, times: RoundTimes) -> Self {
        Self {
            guest: tr.guest.clone(),
            left: (0..tr.scenes.len()).collect(),
            scenes: tr.scenes.clone(),
            up: None,
            played: Vec::new(),
            talk_since: None,
            ballots: 0,
            last_ballot: None,
            times,
        }
    }

    pub fn title(&self, i: usize) -> &str {
        &self.scenes[i].title
    }

    /// The titles of the scenes left, in ballot order (a ballot's options).
    pub fn left_titles(&self) -> Vec<String> {
        self.left
            .iter()
            .map(|&i| self.scenes[i].title.clone())
            .collect()
    }

    /// The scene a closed ballot puts up: the winner; on a tie the tied scene first in ballot
    /// order; with no votes the first scene left.
    pub fn choose(&self, outcome: &PollOutcome) -> Option<usize> {
        let by_title = |t: &str| {
            self.left
                .iter()
                .copied()
                .find(|&i| self.scenes[i].title == t)
        };
        match outcome {
            PollOutcome::Winner(w) => by_title(w).or_else(|| self.left.first().copied()),
            PollOutcome::Tie { options, .. } => self
                .left
                .iter()
                .copied()
                .find(|&i| options.iter().any(|o| o == &self.scenes[i].title)),
            PollOutcome::NoVotes => self.left.first().copied(),
        }
    }

    pub fn cue(&mut self, i: usize, now_ms: TsMs, how: Cue) {
        self.up = Some((i, now_ms, how));
        self.talk_since = None;
    }

    /// The scene up ended: it leaves the ballot and free talk starts. False when `scene_id`
    /// is not the scene up.
    pub fn ended(&mut self, scene_id: &str, now_ms: TsMs) -> bool {
        let Some((i, _, _)) = &self.up else {
            return false;
        };
        let i = *i;
        if self.scenes[i].id != scene_id {
            return false;
        }
        self.up = None;
        self.left.retain(|&k| k != i);
        self.played.push(i);
        self.talk_since = Some(now_ms);
        true
    }

    /// The round in progress (1-based) and the number of rounds.
    pub fn round(&self) -> (usize, usize) {
        let open = usize::from(self.up.is_some());
        (
            (self.played.len() + open).clamp(1, self.scenes.len()),
            self.scenes.len(),
        )
    }

    /// `"A", "B"` (or `none`).
    pub fn quoted(&self, idx: &[usize]) -> String {
        if idx.is_empty() {
            return "none".into();
        }
        idx.iter()
            .map(|&i| format!("\"{}\"", self.scenes[i].title))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// A foil id as a name: `the-painter` -> `the painter`.
pub fn foil_name(id: &str) -> String {
    id.replace(['-', '_'], " ")
}
