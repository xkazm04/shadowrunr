//! Real line timing from a run's `voice.jsonl` (contract § run-directory artifacts).
//!
//! The log says when the stage started playing each part of a line (`played`, on the stream
//! clock) and how long each rendered part runs (`rendered`'s `duration_ms`). A line's real
//! start is its first `played`; its end is its last played part's `played` plus that part's
//! duration. A line with no `played` (the voice skipped it, the stage dropped it, or it was
//! never sent to the voice) is not in the log's timing, and the caller keeps its stream-clock
//! time and flags it unvoiced.

use std::collections::HashMap;
use std::path::Path;

use shadowrunr_types::voice::{VoiceLogEvent, VoiceLogLine};

use crate::{ClipError, Result};

/// The run directory's voice log.
pub const VOICE_LOG: &str = "voice.jsonl";

/// One voiced line on the stream clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heard {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// The heard lines of a run, by the `say` event's seq (its 1-based transcript line).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceTiming {
    heard: HashMap<u64, Heard>,
}

impl VoiceTiming {
    /// The line at transcript line `seq`, when it was heard.
    pub fn heard(&self, seq: u64) -> Option<Heard> {
        self.heard.get(&seq).copied()
    }

    /// Every heard line, by seq.
    pub fn lines(&self) -> impl Iterator<Item = (u64, Heard)> + '_ {
        self.heard.iter().map(|(s, h)| (*s, *h))
    }

    pub fn len(&self) -> usize {
        self.heard.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heard.is_empty()
    }
}

/// Read `<dir>/voice.jsonl`; `None` when the run has none.
pub fn read(dir: &Path) -> Result<Option<VoiceTiming>> {
    let path = dir.join(VOICE_LOG);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ClipError::Io(format!("{}: {e}", path.display())))?;
    Ok(Some(parse(&text)))
}

/// Parse a voice log. A line that does not parse (a torn tail, a future event) is skipped:
/// the log is a record of what happened, and one bad line must not cost the rest.
pub fn parse(text: &str) -> VoiceTiming {
    let mut durations: HashMap<(u64, u32), u64> = HashMap::new();
    // seq -> (first played, last part, that part's played)
    let mut played: HashMap<u64, (i64, u32, i64)> = HashMap::new();
    for line in text.lines() {
        let Ok(l) = serde_json::from_str::<VoiceLogLine>(line) else {
            continue;
        };
        let part = l.part.unwrap_or(0);
        match l.event {
            VoiceLogEvent::Rendered => {
                if let Some(d) = l.duration_ms {
                    durations.insert((l.seq, part), d);
                }
            }
            VoiceLogEvent::Played => {
                let e = played.entry(l.seq).or_insert((l.ts_ms, part, l.ts_ms));
                e.0 = e.0.min(l.ts_ms);
                if part >= e.1 {
                    e.1 = part;
                    e.2 = l.ts_ms;
                }
            }
            _ => {}
        }
    }
    let heard = played
        .into_iter()
        .map(|(seq, (first, last_part, last_at))| {
            let dur = durations.get(&(seq, last_part)).copied().unwrap_or(0) as i64;
            (
                seq,
                Heard {
                    start_ms: first,
                    end_ms: (last_at + dur).max(first + 1),
                },
            )
        })
        .collect();
    VoiceTiming { heard }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_runs_from_its_first_played_to_its_last_parts_end() {
        let log = r#"{"ts_ms":-500,"seq":0,"event":"face","reason":"silhouette"}
{"ts_ms":37008,"seq":16,"event":"rendered","part":0,"duration_ms":834,"text":"Page one."}
{"ts_ms":37010,"seq":16,"event":"rendered","part":1,"duration_ms":1294,"text":"Underlined twice."}
{"ts_ms":37013,"seq":16,"event":"rendered","part":2,"duration_ms":2654,"text":"Apparently."}
{"ts_ms":41632,"seq":16,"event":"played","part":0}
{"ts_ms":42459,"seq":16,"event":"played","part":1}
{"ts_ms":43752,"seq":16,"event":"played","part":2}
{"ts_ms":50000,"seq":17,"event":"rendered","part":0,"duration_ms":900}
{"ts_ms":50001,"seq":17,"event":"skipped","part":0,"reason":"late"}
{"ts_ms":51000,"seq":18,"event":"stage_dropped","reason":"stage_cap"}
{"ts_ms":52000,"seq":19,"event":"played","part":0}
{"ts_ms":5"#;
        let t = parse(log);
        assert_eq!(
            t.heard(16),
            Some(Heard {
                start_ms: 41632,
                end_ms: 43752 + 2654
            })
        );
        // Skipped and dropped lines were never heard.
        assert_eq!(t.heard(17), None);
        assert_eq!(t.heard(18), None);
        // A played part with no rendered duration still has an extent.
        assert_eq!(
            t.heard(19),
            Some(Heard {
                start_ms: 52000,
                end_ms: 52001
            })
        );
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn no_log_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read(dir.path()).unwrap(), None);
    }
}
