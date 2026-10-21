//! The situation bank (`characters/<id>/situations.json`) and its injection schedule.
//!
//! Each situation pairs a behaviour with the response it requires. The simulator schedules
//! every situation into the run and records each attempt, so the judge can tell a rule that
//! held from a rule that was never tested (`not_provoked`).

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;

use serde::{Deserialize, Serialize};

use shadowrunr_types::catalog;
use shadowrunr_types::SegmentId;

use crate::calibration::Calibration;
use crate::population::ArchetypeKind;
use crate::rng::Rng;
use crate::SimError;

/// How a situation is performed: the contract's closed `stimulus` value.
pub use shadowrunr_types::catalog::Stimulus as StimulusKind;

/// One situation of the bank: the contract's [`catalog::Situation`], parsed and validated by
/// the brain's package load, with the sim's scheduling reads on top.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Situation(pub catalog::Situation);

impl Deref for Situation {
    type Target = catalog::Situation;
    fn deref(&self) -> &catalog::Situation {
        &self.0
    }
}

impl DerefMut for Situation {
    fn deref_mut(&mut self) -> &mut catalog::Situation {
        &mut self.0
    }
}

/// Default length of a silence situation, from the bank's "no chat for two minutes".
pub const DEFAULT_SILENCE_S: u32 = 120;

impl Situation {
    /// The stimulus kind: explicit when the bank says so, otherwise inferred from the
    /// archetype, the invariants and the behaviour text.
    pub fn kind(&self) -> StimulusKind {
        if let Some(k) = self.stimulus {
            return k;
        }
        let b = self.behaviour.to_ascii_lowercase();
        let arch = ArchetypeKind::of(&self.archetype);
        if arch.is_wave() {
            StimulusKind::Wave
        } else if arch == ArchetypeKind::Lurker || b.contains("no chat") || b.contains("silence") {
            StimulusKind::Silence
        } else if self.invariants.iter().any(|i| i.contains("vote")) {
            StimulusKind::Vote
        } else {
            StimulusKind::Message
        }
    }

    pub fn silence_len(&self) -> u32 {
        self.silence_s.unwrap_or(DEFAULT_SILENCE_S)
    }

    /// Scheduled in `segment`: every segment when the situation names none.
    pub fn applies_to(&self, segment: SegmentId) -> bool {
        self.segments.is_empty() || self.segments.contains(&segment)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SituationBank {
    pub version: u32,
    pub situations: Vec<Situation>,
}

impl SituationBank {
    /// `<package>/situations.json`, through the brain's package load: one parse, validated
    /// against the package's rules (duplicate ids, unknown fields, stimulus and segment values,
    /// `rule:<id>` invariants). A missing file is an error here: a run needs a bank.
    pub fn load(path: &Path) -> Result<Self, SimError> {
        if !path.is_file() {
            return Err(SimError::Io(format!("{}: not found", path.display())));
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        let bank = shadowrunr_brain::SituationBank::load(dir).map_err(|e| match e {
            shadowrunr_brain::BrainError::Package(s) => SimError::Spec(s),
            other => SimError::Brain(other),
        })?;
        Ok(Self::from(bank))
    }

    pub fn get(&self, id: &str) -> Option<&Situation> {
        self.situations.iter().find(|s| s.id == id)
    }
}

impl From<shadowrunr_brain::SituationBank> for SituationBank {
    fn from(bank: shadowrunr_brain::SituationBank) -> Self {
        Self {
            version: bank.version,
            situations: bank.situations.into_iter().map(Situation).collect(),
        }
    }
}

/// How far a character's bank reaches the calibration's sourced bait tactics: the threat
/// model the simulated audience is meant to carry. A tactic no situation performs is never
/// provoked, so a tournament's zero failures on it are silence, not evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TacticReach {
    pub character: String,
    /// Tactics the calibration documents.
    pub of: usize,
    /// Tactic id -> the bank's situations that perform it.
    pub reached: BTreeMap<String, Vec<String>>,
    /// Tactics no situation performs, in the calibration's order.
    pub unreached: Vec<String>,
}

impl TacticReach {
    /// `bank reach: 4 of 14 tactics (unreached: a, b, ...)`.
    pub fn line(&self) -> String {
        let mut line = format!("bank reach: {} of {} tactics", self.reached.len(), self.of);
        if !self.unreached.is_empty() {
            line.push_str(&format!(" (unreached: {})", self.unreached.join(", ")));
        }
        line
    }
}

/// Bind the calibration's bait tactics to `character`'s bank. Every situation id a tactic
/// binds for this character must be in the bank: a dangling binding is an error, so a renamed
/// situation cannot quietly drop a tactic out of reach.
pub fn tactic_reach(
    bank: &SituationBank,
    character: &str,
    cal: &Calibration,
) -> Result<TacticReach, SimError> {
    let mut reached = BTreeMap::new();
    let mut unreached = Vec::new();
    for t in &cal.bait_tactics {
        let ids = t
            .situations
            .get(character)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some(id) = ids.iter().find(|id| bank.get(id).is_none()) {
            return Err(SimError::Spec(format!(
                "calibration bait_tactics.{}: situations.{character} names {id:?}, which is not in the {character} situation bank",
                t.id
            )));
        }
        if ids.is_empty() {
            unreached.push(t.id.clone());
        } else {
            reached.insert(t.id.clone(), ids.to_vec());
        }
    }
    Ok(TacticReach {
        character: character.to_string(),
        of: cal.bait_tactics.len(),
        reached,
        unreached,
    })
}

/// One scheduled injection and what became of it. Written into `run.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Injection {
    pub situation_id: String,
    pub kind: StimulusKind,
    /// Stream second the injection becomes due.
    pub at_s: u32,
    /// Featured chatter designated to perform it (message / vote kinds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatter_id: Option<String>,
    /// Stream seconds at which the directive was handed to the chatter model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<u32>,
    /// Chat message that performed it, as reported by the simulator at send time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performed_msg_id: Option<String>,
    /// For silence: the window [start, end) in stream seconds during which chat was held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silence_window: Option<(u32, u32)>,
    /// Why it was not (yet) performed, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Left out of the session by the room's closing guard: never delivered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub left_out: bool,
}

impl Injection {
    pub fn attempted(&self) -> bool {
        !self.attempts.is_empty() || self.silence_window.is_some()
    }

    /// Message and vote situations are performed by one featured chatter.
    pub fn needs_chatter(&self) -> bool {
        matches!(self.kind, StimulusKind::Message | StimulusKind::Vote)
    }
}

/// Maximum directive attempts per message/vote situation before giving up.
pub const MAX_ATTEMPTS: usize = 3;

/// Spread every situation of the bank that applies to `segment` across the run.
///
/// `featured` lists `(viewer_id, archetype_kind)` of the featured chatters. A situation goes
/// to a featured chatter of its own archetype when there is one (round-robin among them),
/// otherwise round-robin over everyone featured, so a 3-viewer room still performs every
/// situation.
pub fn schedule(
    bank: &SituationBank,
    segment: SegmentId,
    duration_s: u32,
    featured: &[(String, ArchetypeKind)],
    seed: u64,
) -> Vec<Injection> {
    let mut rng = Rng::derive(seed, "schedule");
    let sits: Vec<&Situation> = bank
        .situations
        .iter()
        .filter(|s| s.applies_to(segment))
        .collect();
    let n = sits.len() as u32;
    if n == 0 {
        return Vec::new();
    }
    let start = (duration_s / 10).min(30);
    let tail = (duration_s / 10).min(30);
    let span = duration_s.saturating_sub(start + tail).max(n);
    let slot = f64::from(span) / f64::from(n);

    let mut order: Vec<usize> = (0..sits.len()).collect();
    rng.shuffle(&mut order);

    let mut rr_all = 0usize;
    let mut rr_kind: Vec<(ArchetypeKind, usize)> = Vec::new();
    let mut out = Vec::with_capacity(order.len());
    for (k, idx) in order.into_iter().enumerate() {
        let s = sits[idx];
        let jitter = rng.range_f64(-0.25, 0.25) * slot;
        let at = (f64::from(start) + (k as f64 + 0.5) * slot + jitter).max(0.0) as u32;
        let kind = s.kind();
        let at = if kind == StimulusKind::Silence {
            // A silence must close before the run ends, or it cannot count as performed.
            at.min(duration_s.saturating_sub(s.silence_len() + 30))
        } else {
            at
        };
        let at = at.min(duration_s.saturating_sub(1));
        let chatter_id = match kind {
            StimulusKind::Silence | StimulusKind::Wave => None,
            _ if featured.is_empty() => None,
            _ => {
                let want = ArchetypeKind::of(&s.archetype);
                let matching: Vec<&String> = featured
                    .iter()
                    .filter(|(_, k)| *k == want)
                    .map(|(id, _)| id)
                    .collect();
                if matching.is_empty() {
                    let id = featured[rr_all % featured.len()].0.clone();
                    rr_all += 1;
                    Some(id)
                } else {
                    let pos = match rr_kind.iter().position(|(k, _)| *k == want) {
                        Some(p) => p,
                        None => {
                            rr_kind.push((want, 0));
                            rr_kind.len() - 1
                        }
                    };
                    let id = matching[rr_kind[pos].1 % matching.len()].clone();
                    rr_kind[pos].1 += 1;
                    Some(id)
                }
            }
        };
        let note = (matches!(kind, StimulusKind::Message | StimulusKind::Vote)
            && chatter_id.is_none())
        .then(|| "no featured chatter available".to_string());
        out.push(Injection {
            situation_id: s.id.clone(),
            kind,
            at_s: at,
            chatter_id,
            attempts: Vec::new(),
            performed_msg_id: None,
            silence_window: None,
            note,
            left_out: false,
        });
    }
    out.sort_by_key(|i| i.at_s);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn malakor_bank() -> SituationBank {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../characters/malakor/situations.json");
        SituationBank::load(&path).unwrap()
    }

    #[test]
    fn kinds_are_inferred() {
        let bank = malakor_bank();
        assert_eq!(
            bank.get("empty-room").unwrap().kind(),
            StimulusKind::Silence
        );
        assert_eq!(
            bank.get("vote-result-unwelcome").unwrap().kind(),
            StimulusKind::Vote
        );
        assert_eq!(
            bank.get("prompt-injection").unwrap().kind(),
            StimulusKind::Message
        );
        assert_eq!(
            bank.get("edgy-roast-request").unwrap().kind(),
            StimulusKind::Message
        );
        assert_eq!(bank.get("raid-wave").unwrap().kind(), StimulusKind::Wave);
        assert_eq!(
            bank.get("copypasta-flood").unwrap().kind(),
            StimulusKind::Wave
        );
    }

    #[test]
    fn schedule_covers_every_situation() {
        let bank = malakor_bank();
        for (duration, featured) in [
            (
                1200u32,
                vec![
                    ("a".to_string(), ArchetypeKind::Baiter),
                    ("b".to_string(), ArchetypeKind::Regular),
                    ("c".to_string(), ArchetypeKind::Newcomer),
                    ("d".to_string(), ArchetypeKind::SubVoter),
                ],
            ),
            // A 3-viewer room with no matching archetypes still covers everything.
            (
                300,
                vec![
                    ("x".to_string(), ArchetypeKind::Lurker),
                    ("y".to_string(), ArchetypeKind::Lurker),
                    ("z".to_string(), ArchetypeKind::Other),
                ],
            ),
            // Absurdly short runs still schedule every situation inside the run.
            (20, vec![("x".to_string(), ArchetypeKind::Regular)]),
        ] {
            let sched = schedule(
                &bank,
                SegmentId::Seg01ImprovisedRoom,
                duration,
                &featured,
                5,
            );
            let applicable: Vec<_> = bank
                .situations
                .iter()
                .filter(|s| s.applies_to(SegmentId::Seg01ImprovisedRoom))
                .collect();
            assert_eq!(sched.len(), applicable.len());
            for s in applicable {
                let inj = sched.iter().find(|i| i.situation_id == s.id).expect(&s.id);
                assert!(
                    inj.at_s < duration,
                    "{} at {} >= {duration}",
                    s.id,
                    inj.at_s
                );
                assert_eq!(inj.chatter_id.is_some(), inj.needs_chatter(), "{}", s.id);
            }
            assert!(sched.windows(2).all(|w| w[0].at_s <= w[1].at_s));
        }
        // Matching archetypes are preferred.
        let featured = vec![
            ("reg".to_string(), ArchetypeKind::Regular),
            ("bait".to_string(), ArchetypeKind::Baiter),
        ];
        let sched = schedule(&bank, SegmentId::Seg01ImprovisedRoom, 1200, &featured, 1);
        let inj = sched
            .iter()
            .find(|i| i.situation_id == "prompt-injection")
            .unwrap();
        assert_eq!(inj.chatter_id.as_deref(), Some("bait"));
        // Two baiters share the baiter situations round-robin.
        let featured = vec![
            ("b1".to_string(), ArchetypeKind::Baiter),
            ("b2".to_string(), ArchetypeKind::Baiter),
        ];
        let sched = schedule(&bank, SegmentId::Seg01ImprovisedRoom, 1200, &featured, 1);
        let baiter_ids: Vec<_> = bank
            .situations
            .iter()
            .filter(|s| s.archetype == "baiter")
            .map(|s| s.id.clone())
            .collect();
        let count = |who: &str| {
            sched
                .iter()
                .filter(|i| {
                    baiter_ids.contains(&i.situation_id) && i.chatter_id.as_deref() == Some(who)
                })
                .count()
        };
        assert!(
            count("b1").abs_diff(count("b2")) <= 1,
            "{} vs {}",
            count("b1"),
            count("b2")
        );
    }

    #[test]
    fn segment_filter() {
        let mut bank = malakor_bank();
        bank.situations[0].segments = vec![SegmentId::Seg02aHexmap];
        let id = bank.situations[0].id.clone();
        let f = vec![("a".to_string(), ArchetypeKind::Regular)];
        let seg01 = schedule(&bank, SegmentId::Seg01ImprovisedRoom, 900, &f, 1);
        assert!(!seg01.iter().any(|i| i.situation_id == id));
        let seg02 = schedule(&bank, SegmentId::Seg02aHexmap, 900, &f, 1);
        assert!(seg02.iter().any(|i| i.situation_id == id));
    }

    #[test]
    fn the_bank_is_the_brain_load() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../characters/malakor");
        let pkg = shadowrunr_brain::load_character(&dir, "").unwrap();
        let bank = malakor_bank();
        let ours: Vec<&catalog::Situation> = bank.situations.iter().map(|s| &s.0).collect();
        let theirs: Vec<&catalog::Situation> = pkg.situations.situations.iter().collect();
        assert_eq!(ours, theirs);
        // A segment-scoped situation stays scoped.
        let s = bank.get("seg02a-raid-subject").unwrap();
        assert!(s.applies_to(SegmentId::Seg02aHexmap));
        assert!(!s.applies_to(SegmentId::Seg01ImprovisedRoom));
    }

    #[test]
    fn a_misspelled_rule_invariant_fails_before_the_run() {
        let d = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../characters/malakor");
        std::fs::create_dir_all(d.path().join("rules")).unwrap();
        for e in std::fs::read_dir(src.join("rules")).unwrap() {
            let p = e.unwrap().path();
            std::fs::copy(&p, d.path().join("rules").join(p.file_name().unwrap())).unwrap();
        }
        let text = std::fs::read_to_string(src.join("situations.json"))
            .unwrap()
            .replace("\"rule:crisis-handoff\"", "\"rule:crisis-handof\"");
        std::fs::write(d.path().join("situations.json"), text).unwrap();
        let e = SituationBank::load(&d.path().join("situations.json")).unwrap_err();
        assert!(e.to_string().contains("unknown rule crisis-handof"), "{e}");
    }

    fn shipped_calibration() -> crate::calibration::Calibration {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sim");
        crate::calibration::load(&p).unwrap().0
    }

    #[test]
    fn a_tactic_bound_to_an_unknown_situation_fails_at_load() {
        let mut cal = shipped_calibration();
        cal.bait_tactics[3]
            .situations
            .insert("malakor".into(), vec!["no-such-situation".into()]);
        let tactic = cal.bait_tactics[3].id.clone();
        let e = tactic_reach(&malakor_bank(), "malakor", &cal).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains(&format!("bait_tactics.{tactic}")), "{msg}");
        assert!(msg.contains("no-such-situation"), "{msg}");
        assert!(msg.contains("malakor"), "{msg}");
    }

    #[test]
    fn the_shipped_bank_reach_names_its_blind_spots() {
        let reach = tactic_reach(&malakor_bank(), "malakor", &shipped_calibration()).unwrap();
        assert_eq!(reach.of, 14);
        for t in [
            "loaded-history-question",
            "payload-split",
            "obfuscated-slur",
            "context-poisoning",
            "spoken-chat-injection",
        ] {
            assert!(reach.unreached.iter().any(|u| u == t), "{t}: {reach:?}");
            assert!(!reach.reached.contains_key(t), "{t}");
        }
        assert_eq!(
            reach.reached["instruction-override"],
            vec!["prompt-injection".to_string()]
        );
        assert_eq!(reach.reached.len() + reach.unreached.len(), reach.of);
        let line = reach.line();
        assert!(
            line.starts_with(&format!(
                "bank reach: {} of 14 tactics",
                reach.reached.len()
            )),
            "{line}"
        );
        assert!(line.contains("loaded-history-question"), "{line}");
        // Another character's bank is reached by nothing the calibration binds.
        let other = tactic_reach(&malakor_bank(), "someone-else", &shipped_calibration()).unwrap();
        assert!(other.reached.is_empty());
        assert_eq!(other.unreached.len(), 14);
    }

    #[test]
    fn schedule_is_deterministic() {
        let bank = malakor_bank();
        let f = vec![("a".to_string(), ArchetypeKind::Regular)];
        let a: Vec<_> = schedule(&bank, SegmentId::Seg02aHexmap, 900, &f, 3)
            .into_iter()
            .map(|i| (i.situation_id, i.at_s))
            .collect();
        let b: Vec<_> = schedule(&bank, SegmentId::Seg02aHexmap, 900, &f, 3)
            .into_iter()
            .map(|i| (i.situation_id, i.at_s))
            .collect();
        assert_eq!(a, b);
    }
}
