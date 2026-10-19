//! The per-beat signals: moment markers (admitted under caps) and the crowd's reaction.
//!
//! **Markers.** The host's `mark_moment` plus three structural beats read from the
//! transcript: a foil line Malakor answers (FOIL_ROAST), a hex reveal (HEX_REVEAL) and a
//! sealed verdict, i.e. a closed vote or a case's last hex annexed (the RELEASE_ANNEXED
//! equivalent). The runtime validates only that a mark's label is non-empty, so a generated
//! host may mark every line; the scorer therefore admits a marker only within caps: a label
//! of at most [`MAX_LABEL_CHARS`], one per [`MARK_COOLDOWN_MS`] per kind and segment, and at
//! most [`MARKS_PER_SEGMENT`] per kind and segment. A capped marker is kept with its reason.
//!
//! **Crowd.** Distinct chatters (not messages) in the [`REACTION_MS`] after a beat, as a
//! z-score against the stream's own trailing baseline (the whole stream until the trailing
//! window holds enough beats). Commands, and messages moderation dropped for any reason other
//! than the per-viewer rate cap, do not count. A flood wave in the reaction window suppresses
//! the signal for that beat: a wave is evidence of an arrival, not of a moment.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use shadowrunr_types::clips::{ClipMarker, MarkerKind};
use shadowrunr_types::{ModStage, Op, StreamEvent, TsMs};

use crate::transcript::{Stream, Who};

/// Longest admitted `mark_moment` label (over-long labels are refused, not truncated).
pub const MAX_LABEL_CHARS: usize = 80;
/// Minimum spacing of admitted markers of one kind within a segment.
pub const MARK_COOLDOWN_MS: i64 = 30_000;
/// Admitted markers per kind and segment.
pub const MARKS_PER_SEGMENT: usize = 6;
/// How far before a mark a chat message asking for a clip counts as the mark's cause.
const CLIP_REQUEST_LOOKBACK_MS: i64 = 15_000;
/// The crowd's reaction window after a beat (simulated chat answers within a tick or two).
pub const REACTION_MS: i64 = 10_000;
/// Trailing baseline for the crowd z-score.
const BASELINE_MS: i64 = 120_000;
const MIN_BASELINE_BEATS: usize = 6;
/// Floor on the baseline's spread, so a room of three does not divide by zero.
const MIN_STD: f64 = 0.5;
/// z at which the crowd signal saturates.
const Z_CAP: f64 = 3.0;

/// What each marker kind is worth. Hypotheses like the fusion weights: the host's own mark
/// carries intent, the structural beats only the show's shape.
pub fn kind_weight(kind: MarkerKind) -> f64 {
    match kind {
        MarkerKind::MarkMoment => 1.0,
        MarkerKind::HexReveal => 0.8,
        MarkerKind::VerdictSealed => 0.7,
        MarkerKind::FoilRoast => 0.6,
    }
}

fn clip_request() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\bclip\s+(?:that|this|it)\b|\bclip\s*it\b|\bclippable\b")
            .expect("clip request pattern compiles")
    })
}

/// Every marker in the stream, chronological, each admitted or capped.
pub fn markers(stream: &Stream) -> Vec<ClipMarker> {
    let mut raw: Vec<(TsMs, MarkerKind, String)> = Vec::new();
    let mut revealed: HashSet<String> = HashSet::new();
    for (i, (_, ev)) in stream.events.iter().enumerate() {
        match ev {
            StreamEvent::Op { ts_ms, op } => match op {
                Op::MarkMoment { label } => {
                    raw.push((*ts_ms, MarkerKind::MarkMoment, label.clone()))
                }
                Op::RevealHex { hex, grade } => {
                    revealed.insert(hex.clone());
                    let label = stream
                        .case_at(*ts_ms)
                        .and_then(|c| c.hexes.iter().find(|h| &h.id == hex))
                        .map(|h| format!("{hex} revealed ({}): {}", grade_str(*grade), h.label))
                        .unwrap_or_else(|| format!("{hex} revealed ({})", grade_str(*grade)));
                    raw.push((*ts_ms, MarkerKind::HexReveal, label));
                    if let Some(case) = stream.case_at(*ts_ms) {
                        if !case.hexes.is_empty()
                            && case.hexes.iter().all(|h| revealed.contains(&h.id))
                        {
                            raw.push((
                                *ts_ms,
                                MarkerKind::VerdictSealed,
                                format!("Case {} fully annexed", case.id),
                            ));
                        }
                    }
                }
                _ => {}
            },
            StreamEvent::Vote {
                ts_ms,
                poll_id,
                tallies,
                closed: true,
            } => {
                if let Some(win) = tallies
                    .iter()
                    .filter(|t| t.voters > 0)
                    .max_by(|a, b| a.weight.total_cmp(&b.weight))
                {
                    raw.push((
                        *ts_ms,
                        MarkerKind::VerdictSealed,
                        format!("Vote {poll_id} sealed: {}", win.option),
                    ));
                }
            }
            StreamEvent::Say {
                ts_ms,
                speaker: shadowrunr_types::Speaker::Foil,
                foil,
                ..
            } => {
                // Answered by Malakor later in this beat or in the next one.
                let answered = stream.events[i + 1..]
                    .iter()
                    .filter_map(|(_, e)| match e {
                        StreamEvent::Say {
                            ts_ms: t,
                            speaker: shadowrunr_types::Speaker::Malakor,
                            ..
                        } => Some(*t),
                        _ => None,
                    })
                    .next()
                    .filter(|t| *t - *ts_ms <= 5_000);
                if let Some(t) = answered {
                    let who = foil.clone().unwrap_or_else(|| "the foil".into());
                    raw.push((t, MarkerKind::FoilRoast, format!("Malakor answers {who}")));
                }
            }
            _ => {}
        }
    }
    raw.sort_by_key(|(ts, _, _)| *ts);
    admit(stream, raw)
}

fn grade_str(g: shadowrunr_types::EvidenceGrade) -> &'static str {
    match g {
        shadowrunr_types::EvidenceGrade::Doc => "DOC",
        shadowrunr_types::EvidenceGrade::Rep => "REP",
        shadowrunr_types::EvidenceGrade::Clm => "CLM",
        shadowrunr_types::EvidenceGrade::Spc => "SPC",
    }
}

fn admit(stream: &Stream, raw: Vec<(TsMs, MarkerKind, String)>) -> Vec<ClipMarker> {
    // (segment, kind) -> (last admitted ts, admitted count)
    let mut state: HashMap<(String, MarkerKind), (TsMs, usize)> = HashMap::new();
    let mut out = Vec::new();
    for (ts, kind, label) in raw {
        let label = label.trim().to_string();
        let seg = stream
            .segment_at(ts)
            .map(|s| format!("{s:?}"))
            .unwrap_or_default();
        let (last, count) = state
            .get(&(seg.clone(), kind))
            .copied()
            .unwrap_or((i64::MIN / 2, 0));
        let capped = if label.is_empty() {
            Some("label_empty")
        } else if kind == MarkerKind::MarkMoment && label.chars().count() > MAX_LABEL_CHARS {
            Some("label_too_long")
        } else if ts - last < MARK_COOLDOWN_MS {
            Some("cooldown")
        } else if count >= MARKS_PER_SEGMENT {
            Some("segment_ceiling")
        } else {
            None
        };
        if capped.is_none() {
            state.insert((seg, kind), (ts, count + 1));
        }
        let chat_requested = kind == MarkerKind::MarkMoment
            && stream.chats.values().any(|c| {
                c.ts_ms <= ts
                    && ts - c.ts_ms <= CLIP_REQUEST_LOOKBACK_MS
                    && clip_request().is_match(&c.text)
            });
        out.push(ClipMarker {
            kind,
            ts_ms: ts,
            label,
            admitted: capped.is_none(),
            capped: capped.map(String::from),
            chat_requested,
        });
    }
    out
}

/// The crowd signal of one beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Crowd {
    Measured {
        z: f64,
        value: f64,
    },
    /// A flood wave in the reaction window.
    Wave,
}

/// The crowd signal of every beat (index-aligned with `stream.beats`).
pub fn crowd(stream: &Stream) -> Vec<Crowd> {
    let excluded: HashSet<&str> = stream
        .mods
        .iter()
        .filter(|m| m.stage == ModStage::Input && m.action == "dropped" && m.rule != "rate_cap")
        .filter_map(|m| m.msg_id.as_deref())
        .collect();
    let waves: Vec<TsMs> = stream
        .mods
        .iter()
        .filter(|m| m.rule == "flood_cluster" || m.rule == "copypasta_cluster")
        .map(|m| m.ts_ms)
        .collect();
    let mut chats: Vec<(TsMs, &str)> = stream
        .chats
        .iter()
        .filter(|(id, c)| !c.is_command && !excluded.contains(id.as_str()))
        .map(|(_, c)| (c.ts_ms, c.viewer_id.as_str()))
        .collect();
    chats.sort();
    let counts: Vec<f64> = stream
        .beats
        .iter()
        .map(|b| {
            let lo = chats.partition_point(|(t, _)| *t <= b.ts_ms);
            let hi = chats.partition_point(|(t, _)| *t <= b.ts_ms + REACTION_MS);
            chats[lo..hi]
                .iter()
                .map(|(_, v)| *v)
                .collect::<HashSet<_>>()
                .len() as f64
        })
        .collect();
    let whole = mean_std(&counts);
    stream
        .beats
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if waves
                .iter()
                .any(|t| *t > b.ts_ms && *t <= b.ts_ms + REACTION_MS)
            {
                return Crowd::Wave;
            }
            let trailing: Vec<f64> = stream.beats[..i]
                .iter()
                .zip(&counts)
                .filter(|(p, _)| b.ts_ms - p.ts_ms <= BASELINE_MS)
                .map(|(_, c)| *c)
                .collect();
            let (mean, std) = if trailing.len() >= MIN_BASELINE_BEATS {
                mean_std(&trailing)
            } else {
                whole
            };
            let z = (counts[i] - mean) / std.max(MIN_STD);
            Crowd::Measured {
                z,
                value: z.clamp(0.0, Z_CAP) / Z_CAP,
            }
        })
        .collect()
}

fn mean_std(xs: &[f64]) -> (f64, f64) {
    if xs.is_empty() {
        return (0.0, MIN_STD);
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    (mean, var.sqrt())
}

/// The strongest admitted marker in a beat's tick (`[beat, next beat)`), with its index.
pub fn beat_marker(stream: &Stream, beat: usize, markers: &[ClipMarker]) -> Option<usize> {
    let from = stream.beats[beat].ts_ms;
    let to = stream.beats.get(beat + 1).map_or(i64::MAX, |b| b.ts_ms);
    markers
        .iter()
        .enumerate()
        .filter(|(_, m)| m.admitted && m.ts_ms >= from && m.ts_ms < to)
        .max_by(|(_, a), (_, b)| kind_weight(a.kind).total_cmp(&kind_weight(b.kind)))
        .map(|(i, _)| i)
}

/// Whether a line is spoken by the host or a foil (not a viewer cue).
pub fn is_host_line(who: &Who) -> bool {
    !matches!(who, Who::Viewer { .. })
}
