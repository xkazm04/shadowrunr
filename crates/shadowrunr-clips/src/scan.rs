//! A scan: transcript in, the stream's best five candidates out, each drafted, gated and
//! stored at `pending_owner_gate`.
//!
//! 1. Every beat is pre-scored from its admitted marker and its crowd reaction.
//! 2. The best [`PRE_CANDIDATES`] beats, at least [`MIN_SPACING_MS`] apart, are cut.
//! 3. One hook read rates those cuts (one engine call per run, cheapest tier); when the run
//!    has a recording, each cut's programme loudness is measured too (the audio signal).
//! 4. The fused score ranks them; overlapping cuts merge when the merge fits the band;
//!    the top [`crate::TOP_PER_STREAM`] are drafted and gated.
//!
//! A run with a `voice.jsonl` is cut on the voice's real timing ([`Stream::apply_voice`]).
//!
//! A rescan replaces the run's untouched candidates and keeps every one the owner has
//! already approved or rejected, and every recut the owner made ([`recut`]); watched feedback
//! carries over to the same cut.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use regex::Regex;
use shadowrunr_brain::moderation::Policy;
use shadowrunr_types::clips::{
    ClipCandidate, ClipCaption, ClipEvidence, ClipHex, ClipLane, ClipMarker, ClipRecording,
    ClipScanReport, ClipScanRun, ClipScoring, ClipSignals, ClipSource, ClipStatus, ClipTransition,
    ClipWeights, ClipWindow,
};
use shadowrunr_types::{Engine, EvidenceGrade, Op, SegmentId, StreamEvent};

use crate::footage;
use crate::hook::{self, HookWindow};
use crate::host::ClipHost;
use crate::render::{self, header_for, ClipInput, DISCLOSURE};
use crate::signals::{self, kind_weight, Crowd};
use crate::store::{self, ClipStore};
use crate::transcript::{self, Stream};
use crate::voice;
use crate::window::{self, band_for, Cut};
use crate::{
    clock, gate, now_ms, Result, NOMINAL_WEIGHTS, TOP_PER_STREAM, WEIGHTS_NOTE, WEIGHTS_PROVENANCE,
};

/// Beats cut and sent to the hook read per run (bounds the read's size and cost).
pub const PRE_CANDIDATES: usize = 8;
/// Pre-candidates are at least this far apart on the stream clock.
pub const MIN_SPACING_MS: i64 = 30_000;
/// A signal "fires" (agrees) at or above these levels.
const MARKER_FIRES: f64 = 0.5;
const CHAT_FIRES: f64 = 0.5;
const HOOK_FIRES: f64 = 0.6;

/// One run to scan.
#[derive(Debug, Clone)]
pub struct RunInput {
    pub tournament_id: Option<String>,
    pub run_id: String,
    /// The run directory (holds `transcript.jsonl`).
    pub dir: PathBuf,
    /// The transcript path as recorded on candidates (relative to the runs root).
    pub transcript_label: String,
    /// Who the run's host is (its character's persona): captions, the hook read and the
    /// draft's `clip.json` name them.
    pub host: ClipHost,
}

#[derive(Clone)]
pub struct ScanOptions {
    /// Ask the engine for hook reads.
    pub hook: bool,
    pub policy: Arc<Policy>,
    /// The brain's care window.
    pub care_window_ms: i64,
    /// ffmpeg, for the audio signal of a recorded run; `None` leaves audio unmeasured.
    pub ffmpeg: Option<String>,
}

/// One cut being scored.
#[derive(Debug, Clone)]
struct Pre {
    beat: usize,
    /// The payoff line (index into `stream.lines`).
    payoff: usize,
    marker: Option<usize>,
    crowd: Crowd,
    segment: Option<SegmentId>,
    cut: Cut,
    hook: Option<(f64, String)>,
    /// `Ok((value, window dBFS))`, or why the loudness is unmeasured.
    audio: std::result::Result<(f64, f64), String>,
    score: f64,
}

fn weights_for(chat: bool, audio: bool, hook: bool) -> ClipWeights {
    let w = NOMINAL_WEIGHTS;
    let on = |b: bool, x: f64| if b { x } else { 0.0 };
    let denom = w.marker + on(chat, w.chat) + on(audio, w.audio) + on(hook, w.hook);
    ClipWeights {
        marker: w.marker / denom,
        chat: on(chat, w.chat) / denom,
        audio: on(audio, w.audio) / denom,
        hook: on(hook, w.hook) / denom,
    }
}

fn marker_value(markers: &[ClipMarker], m: Option<usize>) -> f64 {
    m.map_or(0.0, |i| kind_weight(markers[i].kind))
}

fn crowd_value(c: Crowd) -> Option<f64> {
    match c {
        Crowd::Measured { value, .. } => Some(value),
        Crowd::Wave => None,
    }
}

fn fuse(
    marker: f64,
    chat: Option<f64>,
    audio: Option<f64>,
    hook: Option<f64>,
) -> (f64, ClipWeights) {
    let w = weights_for(chat.is_some(), audio.is_some(), hook.is_some());
    let s = w.marker * marker
        + w.chat * chat.unwrap_or(0.0)
        + w.audio * audio.unwrap_or(0.0)
        + w.hook * hook.unwrap_or(0.0);
    ((s * 1000.0).round() / 1000.0, w)
}

/// The recorded run's level context: the recording, its ffmpeg and the whole programme's RMS
/// level, or why audio is unmeasured for the run.
struct Audio<'a> {
    rec: &'a ClipRecording,
    ffmpeg: &'a str,
    stream_db: f64,
}

fn audio_context<'a>(
    rec: Option<&'a ClipRecording>,
    ffmpeg: Option<&'a str>,
) -> std::result::Result<Audio<'a>, String> {
    let rec = rec.ok_or("the run has no recording (recording.json)")?;
    let ffmpeg = ffmpeg.ok_or("no ffmpeg configured for the loudness read")?;
    let stream_db = footage::rms_db(ffmpeg, std::path::Path::new(&rec.file), None)
        .map_err(|e| format!("the recording's level could not be read: {e}"))?;
    Ok(Audio {
        rec,
        ffmpeg,
        stream_db,
    })
}

/// One cut's RMS level against the stream's.
fn audio_of(a: &Audio, cut: &Cut) -> std::result::Result<(f64, f64), String> {
    let from = cut.spoken_start - a.rec.stream_offset_ms;
    let db = footage::rms_db(
        a.ffmpeg,
        std::path::Path::new(&a.rec.file),
        Some((from, cut.duration_ms())),
    )?;
    Ok((footage::audio_value(db, a.stream_db), db))
}

/// Handles of everyone who chatted, as one case-insensitive whole-word pattern.
fn redactor(names: &[String]) -> Option<Regex> {
    let mut names: Vec<&String> = names
        .iter()
        .filter(|n| n.chars().count() >= 3)
        .filter(|n| {
            let w = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
            w(n.chars().next()) && w(n.chars().next_back())
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let alt: Vec<String> = names.iter().map(|n| regex::escape(n)).collect();
    Regex::new(&format!(r"(?i)@?\b(?:{})\b", alt.join("|"))).ok()
}

fn redact(re: Option<&Regex>, text: &str) -> String {
    match re {
        Some(r) => r.replace_all(text, "[viewer]").into_owned(),
        None => text.to_string(),
    }
}

fn captions(stream: &Stream, cut: &Cut, policy: &Policy, re: Option<&Regex>) -> Vec<ClipCaption> {
    let lines = &stream.lines[cut.first..=cut.last];
    lines
        .iter()
        .enumerate()
        .map(|(k, l)| {
            let next = lines.get(k + 1).map_or(cut.spoken_end, |n| n.spoken_start);
            let start = l.spoken_start.max(cut.spoken_start);
            let end = (l.spoken_end + 1_200)
                .min(next)
                .min(cut.spoken_end)
                .max(l.spoken_end.min(cut.spoken_end))
                .max(start);
            ClipCaption {
                start_ms: start - cut.spoken_start,
                end_ms: end - cut.spoken_start,
                speaker: l.who.label(),
                text: policy.bleep(&redact(re, &l.text)),
                unvoiced: l.unvoiced,
            }
        })
        .collect()
}

fn severity(g: EvidenceGrade) -> u8 {
    match g {
        EvidenceGrade::Doc => 0,
        EvidenceGrade::Rep => 1,
        EvidenceGrade::Clm => 2,
        EvidenceGrade::Spc => 3,
    }
}

/// The claim context of a SEG-02A cut: hexes it reveals or names, their grades so far, and
/// their first sources.
fn evidence(stream: &Stream, cut: &Cut, anchor_ts: i64) -> Option<ClipEvidence> {
    let case = stream.case_at(anchor_ts)?;
    let lines = &stream.lines[cut.first..=cut.last];
    let (from_ev, to_ev) = (lines[0].event, lines[lines.len() - 1].event);
    let end_ts = lines[lines.len() - 1].ts_ms;
    let mut ids: Vec<String> = Vec::new();
    for (i, (_, ev)) in stream.events.iter().enumerate() {
        if i < from_ev || i > to_ev {
            continue;
        }
        if let StreamEvent::Op {
            op: Op::RevealHex { hex, .. },
            ..
        } = ev
        {
            if !ids.contains(hex) {
                ids.push(hex.clone());
            }
        }
    }
    for l in lines {
        let lower = l.text.to_lowercase();
        for h in &case.hexes {
            let named = lower.contains(&h.id.to_lowercase())
                || (h.label.chars().count() >= 8 && lower.contains(&h.label.to_lowercase()));
            if named && !ids.contains(&h.id) {
                ids.push(h.id.clone());
            }
        }
    }
    let grade_of = |id: &str| {
        stream
            .events
            .iter()
            .rev()
            .filter_map(|(_, e)| match e {
                StreamEvent::Op {
                    ts_ms,
                    op: Op::RevealHex { hex, grade },
                } if hex == id && *ts_ms <= end_ts => Some(*grade),
                _ => None,
            })
            .next()
    };
    let hexes: Vec<ClipHex> = ids
        .iter()
        .filter_map(|id| case.hexes.iter().find(|h| &h.id == id))
        .map(|h| ClipHex {
            id: h.id.clone(),
            label: h.label.clone(),
            grade: grade_of(&h.id),
            sources: h.sources.clone(),
        })
        .collect();
    let worst_grade = hexes
        .iter()
        .filter_map(|h| h.grade)
        .max_by_key(|g| severity(*g));
    let mut citation: Vec<String> = Vec::new();
    for h in &hexes {
        if let Some(s) = h.sources.first() {
            if !citation.contains(s) && citation.len() < 3 {
                citation.push(s.clone());
            }
        }
    }
    Some(ClipEvidence {
        case_id: case.id.clone(),
        case_title: case.title.clone(),
        fictional: case.fictional,
        hexes,
        worst_grade,
        citation,
    })
}

/// The scene events of the cut on the clip clock: the state before it (case, segment,
/// revealed hexes, the foil on stage, the screen up, the records shown and the strings pinned)
/// at 0, then the window's ops, strings and votes. Never chat or
/// lines: captions carry the lines, and handles stay in the room.
fn clip_events(stream: &Stream, cut: &Cut, re: Option<&Regex>) -> Vec<StreamEvent> {
    let first_ev = stream.lines[cut.first].event;
    let last_ev = stream.lines[cut.last].event;
    let mut out = Vec::new();
    for (i, (_, ev)) in stream.events.iter().enumerate() {
        let before = i < first_ev;
        if i > last_ev {
            break;
        }
        let at = if before {
            0
        } else {
            (stream.spoken_at[i] - cut.spoken_start).clamp(0, cut.duration_ms())
        };
        let keep = match ev {
            // The screen up when the cut starts, every record it may open (records are staged
            // case data, never chat, so they are not redacted), and the board's strings: the
            // ones pinned before the cut stand at 0, the cut's own are drawn in.
            StreamEvent::SegmentStart { .. }
            | StreamEvent::Handover { .. }
            | StreamEvent::CaseOpen { .. }
            | StreamEvent::RecordShown { .. }
            | StreamEvent::LinkPinned { .. } => true,
            StreamEvent::Op { op, .. } => match op {
                Op::RevealHex { .. }
                | Op::SummonFoil { .. }
                | Op::DismissFoil
                | Op::ShowScreen { .. } => true,
                Op::ShowCard { .. } | Op::StartPoll { .. } | Op::ClosePoll { .. } => !before,
                _ => false,
            },
            StreamEvent::Vote { .. } => !before,
            _ => false,
        };
        if !keep {
            continue;
        }
        let mut ev = ev.clone();
        set_ts(&mut ev, at);
        if let StreamEvent::Op {
            op: Op::ShowCard { title, body, .. },
            ..
        } = &mut ev
        {
            *title = redact(re, title);
            *body = redact(re, body);
        }
        out.push(ev);
    }
    out
}

fn set_ts(ev: &mut StreamEvent, at: i64) {
    match ev {
        StreamEvent::StreamStart { ts_ms, .. }
        | StreamEvent::SegmentStart { ts_ms, .. }
        | StreamEvent::SegmentEnd { ts_ms, .. }
        | StreamEvent::CaseOpen { ts_ms, .. }
        | StreamEvent::SceneStart { ts_ms, .. }
        | StreamEvent::SceneEnd { ts_ms, .. }
        | StreamEvent::RecordShown { ts_ms, .. }
        | StreamEvent::LinkPinned { ts_ms, .. }
        | StreamEvent::Chat { ts_ms, .. }
        | StreamEvent::Picked { ts_ms, .. }
        | StreamEvent::Say { ts_ms, .. }
        | StreamEvent::Op { ts_ms, .. }
        | StreamEvent::Vote { ts_ms, .. }
        | StreamEvent::Moderation { ts_ms, .. }
        | StreamEvent::Handover { ts_ms, .. }
        | StreamEvent::Job { ts_ms, .. }
        | StreamEvent::Health { ts_ms, .. }
        | StreamEvent::BeatPlan { ts_ms, .. }
        | StreamEvent::StreamEnd { ts_ms, .. } => *ts_ms = at,
    }
}

fn seg_name(s: Option<SegmentId>) -> String {
    s.and_then(|s| serde_json::to_value(s).ok())
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "none".into())
}

fn overlaps(a: &Cut, b: &Cut) -> bool {
    a.first <= b.last && b.first <= a.last
}

/// Score, cut and rank one stream. Returns the kept cuts best first, plus the markers and
/// the hook outcome.
#[allow(clippy::too_many_arguments)]
async fn rank(
    stream: &Stream,
    run_id: &str,
    host: &ClipHost,
    engine: Option<&dyn Engine>,
    opts: &ScanOptions,
    re: Option<&Regex>,
    audio: &std::result::Result<Audio<'_>, String>,
) -> (Vec<Pre>, Vec<ClipMarker>, String) {
    let markers = signals::markers(stream);
    let crowd = signals::crowd(stream);
    let mut pres: Vec<Pre> = Vec::new();
    for (b, beat) in stream.beats.iter().enumerate() {
        let host = stream.lines[beat.first..=beat.last]
            .iter()
            .any(|l| signals::is_host_line(&l.who));
        if !host {
            continue;
        }
        let marker = signals::beat_marker(stream, b, &markers);
        let (pre, _) = fuse(
            marker_value(&markers, marker),
            crowd_value(crowd[b]),
            None,
            None,
        );
        if pre <= 0.0 {
            continue;
        }
        let segment = stream.segment_at(beat.ts_ms);
        let payoff = window::payoff_line(
            stream,
            b,
            marker.map(|i| (&markers[i].kind, markers[i].label.as_str())),
        );
        pres.push(Pre {
            beat: b,
            payoff,
            marker,
            crowd: crowd[b],
            segment,
            cut: window::cut(stream, payoff, band_for(segment)),
            hook: None,
            audio: Err(String::new()),
            score: pre,
        });
    }
    pres.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.beat.cmp(&b.beat)));
    let mut picked: Vec<Pre> = Vec::new();
    for p in pres {
        let ts = stream.beats[p.beat].ts_ms;
        if picked
            .iter()
            .all(|q| (stream.beats[q.beat].ts_ms - ts).abs() >= MIN_SPACING_MS)
        {
            picked.push(p);
        }
        if picked.len() == PRE_CANDIDATES {
            break;
        }
    }

    // The hook read: one call for the run.
    let hook_state = if !opts.hook {
        "off".to_string()
    } else if let Some(engine) = engine {
        let windows: Vec<HookWindow> = picked
            .iter()
            .enumerate()
            .map(|(k, p)| HookWindow {
                id: format!("w{}", k + 1),
                lines: captions(stream, &p.cut, &opts.policy, re)
                    .into_iter()
                    .map(|c| format!("{}: {}", c.speaker, c.text))
                    .collect(),
            })
            .collect();
        match hook::read(engine, run_id, &windows, host).await {
            Ok(scores) => {
                for (k, p) in picked.iter_mut().enumerate() {
                    if let Some(s) = scores.get(&format!("w{}", k + 1)) {
                        p.hook = Some((s.hook, s.why.clone()));
                    }
                }
                "measured".to_string()
            }
            Err(why) => format!("unmeasured: {why}"),
        }
    } else {
        "unmeasured: no engine".to_string()
    };

    for p in &mut picked {
        p.audio = match audio {
            Ok(a) => audio_of(a, &p.cut).map_err(|e| format!("the window's level: {e}")),
            Err(why) => Err(why.clone()),
        };
        let (s, _) = fuse(
            marker_value(&markers, p.marker),
            crowd_value(p.crowd),
            p.audio.as_ref().ok().map(|a| a.0),
            p.hook.as_ref().map(|h| h.0),
        );
        p.score = s;
    }
    picked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.beat.cmp(&b.beat)));

    // Overlapping cuts: merge when the union fits the band, else the better one stands.
    let mut kept: Vec<Pre> = Vec::new();
    for p in picked {
        if let Some(k) = kept.iter_mut().find(|k| overlaps(&k.cut, &p.cut)) {
            let band = band_for(k.segment);
            let first = k.cut.first.min(p.cut.first);
            let last = k.cut.last.max(p.cut.last);
            let start = window::edge_start(stream, first);
            let end = k.cut.spoken_end.max(p.cut.spoken_end);
            if k.segment == p.segment && end - start <= band.max_ms {
                let payoff = stream.lines[k.payoff].spoken_start;
                k.cut = Cut {
                    first,
                    last,
                    spoken_start: start,
                    spoken_end: end,
                    preroll_ms: payoff - start,
                    needs_reauthoring: k.cut.needs_reauthoring,
                    note: Some("merged with an overlapping candidate".into()),
                };
            }
            continue;
        }
        kept.push(p);
        if kept.len() == TOP_PER_STREAM {
            break;
        }
    }
    (kept, markers, hook_state)
}

/// One scanned run's context for its candidates.
struct RunCtx<'a> {
    input: &'a RunInput,
    recording: Option<&'a ClipRecording>,
    stream_db: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
fn candidate(
    stream: &Stream,
    run: &RunCtx,
    p: &Pre,
    rank: u32,
    markers: &[ClipMarker],
    policy: &Policy,
    re: Option<&Regex>,
    now: i64,
) -> (ClipCandidate, ClipInput) {
    let input = run.input;
    let lines = &stream.lines[p.cut.first..=p.cut.last];
    let beat = &stream.beats[p.beat];
    let band = band_for(p.segment);
    let marker = p.marker.map(|i| &markers[i]);
    let title = match marker {
        Some(m) => redact(re, &m.label),
        None => redact(re, &stream.lines[p.payoff].text),
    };
    let title: String = title.chars().take(90).collect();
    let crowd_z = match p.crowd {
        Crowd::Measured { z, .. } => Some((z * 100.0).round() / 100.0),
        Crowd::Wave => None,
    };
    let mut signals = ClipSignals {
        marker: marker_value(markers, p.marker),
        chat: crowd_value(p.crowd).map(|v| (v * 1000.0).round() / 1000.0),
        chat_z: crowd_z,
        audio: p.audio.as_ref().ok().map(|a| a.0),
        audio_db: p.audio.as_ref().ok().map(|a| (a.1 * 10.0).round() / 10.0),
        audio_stream_db: p
            .audio
            .as_ref()
            .ok()
            .and(run.stream_db)
            .map(|l| (l * 10.0).round() / 10.0),
        hook: p.hook.as_ref().map(|h| (h.0 * 1000.0).round() / 1000.0),
        hook_why: p
            .hook
            .as_ref()
            .map(|h| h.1.clone())
            .filter(|w| !w.is_empty()),
        unmeasured: Default::default(),
    };
    if let Err(why) = &p.audio {
        signals.unmeasured.insert("audio".into(), why.clone());
    }
    if p.crowd == Crowd::Wave {
        signals.unmeasured.insert(
            "chat".into(),
            "a flood wave in the reaction window (evidence of an arrival, not a moment)".into(),
        );
    }
    if p.hook.is_none() {
        signals
            .unmeasured
            .insert("hook".into(), "no hook read for this window".into());
    }
    let (_, effective) = fuse(signals.marker, signals.chat, signals.audio, signals.hook);
    let mut agreeing = Vec::new();
    if signals.marker >= MARKER_FIRES {
        agreeing.push("marker".to_string());
    }
    if signals.chat.is_some_and(|c| c >= CHAT_FIRES) {
        agreeing.push("chat".to_string());
    }
    if signals.hook.is_some_and(|h| h >= HOOK_FIRES) {
        agreeing.push("hook".to_string());
    }
    let lane = if agreeing.len() >= 2 && !p.cut.needs_reauthoring {
        ClipLane::Clip
    } else {
        ClipLane::Review
    };
    let first = &lines[0];
    let last = &lines[lines.len() - 1];
    let seed = format!(
        "{}/{}/{}-{}",
        input.tournament_id.as_deref().unwrap_or(""),
        input.run_id,
        first.line_no,
        last.line_no
    );
    let id = format!(
        "clip-{:016x}",
        shadowrunr_brain::util::fnv1a64(seed.as_bytes())
    );
    let in_window: Vec<ClipMarker> = markers
        .iter()
        .filter(|m| m.ts_ms >= first.ts_ms && m.ts_ms <= last.ts_ms)
        .cloned()
        .map(|mut m| {
            m.label = redact(re, &m.label);
            m
        })
        .collect();
    // The evidence checklist is the investigation's: SEG-01's improv and SEG-03's original,
    // fictional scenes make no claims about real entities.
    let mandatory = p.segment == Some(SegmentId::Seg02aHexmap);
    let evidence = if mandatory {
        evidence(stream, &p.cut, beat.ts_ms)
    } else {
        None
    };
    let caps = captions(stream, &p.cut, policy, re);
    let (driver, lag) = if p.marker.is_some() {
        ("marker", 0)
    } else {
        ("crowd", signals::REACTION_MS / 2)
    };
    let window = ClipWindow {
        anchor_ms: beat.ts_ms,
        start_ms: first.ts_ms,
        end_ms: last.ts_ms,
        spoken_start_ms: p.cut.spoken_start,
        spoken_end_ms: p.cut.spoken_end,
        duration_s: (p.cut.duration_ms() as f64 / 10.0).round() / 100.0,
        driver: driver.into(),
        lag_ms: lag,
        preroll_ms: p.cut.preroll_ms,
        band_s: [(band.min_ms / 1000) as u32, (band.max_ms / 1000) as u32],
        style: band.style.into(),
        needs_reauthoring: p.cut.needs_reauthoring,
        note: p.cut.note.clone(),
        voice_timed: stream.voice_timed,
    };
    let input_json = ClipInput {
        version: 1,
        id: id.clone(),
        width: crate::WIDTH,
        height: crate::HEIGHT,
        duration_ms: p.cut.duration_ms(),
        style: band.style.into(),
        segment: seg_name(p.segment),
        title: title.clone(),
        header: header_for(evidence.as_ref()),
        captions: caps.clone(),
        events: clip_events(stream, &p.cut, re),
        case: if mandatory {
            stream.case_at(beat.ts_ms).cloned()
        } else {
            None
        },
        disclosure: DISCLOSURE.into(),
        host: input.host.clone(),
        provenance: format!(
            "{} · stream {}-{}",
            input.run_id,
            clock(first.ts_ms),
            clock(last.ts_ms)
        ),
    };
    let c = ClipCandidate {
        id,
        created_ms: now,
        status: ClipStatus::RenderedDraft,
        score: p.score,
        rank,
        lane,
        segment: p.segment.unwrap_or(SegmentId::Seg01ImprovisedRoom),
        title,
        source: ClipSource {
            tournament_id: input.tournament_id.clone(),
            run_id: input.run_id.clone(),
            stream_id: stream.stream_id.clone(),
            transcript: input.transcript_label.clone(),
            line_from: first.line_no,
            line_to: last.line_no,
        },
        window,
        markers: in_window,
        signals,
        scoring: ClipScoring {
            nominal: NOMINAL_WEIGHTS,
            effective,
            provenance: if p.audio.is_ok() {
                format!("{WEIGHTS_PROVENANCE}; audio: {}", footage::AUDIO_ASSUMPTION)
            } else {
                WEIGHTS_PROVENANCE.into()
            },
            agreeing,
        },
        linked_case_id: evidence.as_ref().map(|e| e.case_id.clone()),
        evidence,
        captions: caps,
        render: None,
        gate: gate::combine(mandatory, Vec::new(), now),
        approval: None,
        rejection: None,
        history: Vec::new(),
        dry_runs: Vec::new(),
        recording: run.recording.cloned(),
        feedback: None,
        acts: None,
    };
    (c, input_json)
}

/// Gate a candidate: its stored exclusions plus the render checks and the corrections log.
pub fn regate(
    store: &ClipStore,
    c: &mut ClipCandidate,
    exclusions: Vec<shadowrunr_types::clips::GateCheck>,
) {
    let required = c.gate.requires_mandatory_gate;
    let mut checks = exclusions;
    checks.extend(gate::render_checks(
        c.render.as_ref(),
        c.evidence.as_ref(),
        required,
    ));
    if required {
        checks.push(gate::corrections_check(
            &store.data_dir(),
            c.evidence.as_ref(),
        ));
    }
    c.gate = gate::combine(required, checks, now_ms());
}

/// The exclusion checks stored on a candidate (they come from the transcript and do not
/// change with a re-render).
pub fn stored_exclusions(c: &ClipCandidate) -> Vec<shadowrunr_types::clips::GateCheck> {
    const RENDER: [&str; 5] = [
        "ai_disclosure",
        "handles_in_picture",
        "evidence_badge_burnt",
        "source_citation",
        "no_open_correction",
    ];
    c.gate
        .checks
        .iter()
        .filter(|k| !RENDER.contains(&k.id.as_str()))
        .cloned()
        .collect()
}

/// Scan `runs` into the store.
pub async fn scan(
    store: &ClipStore,
    runs: &[RunInput],
    engine: Option<&dyn Engine>,
    opts: &ScanOptions,
) -> Result<ClipScanReport> {
    let mut report = ClipScanReport {
        runs: Vec::new(),
        written: 0,
        kept: 0,
        replaced: 0,
        top: Vec::new(),
        weights_note: WEIGHTS_NOTE.into(),
    };
    let mut fresh: Vec<ClipCandidate> = Vec::new();
    for input in runs {
        let events = transcript::read_events(&input.dir.join("transcript.jsonl"))?;
        let mut stream = Stream::build(&input.run_id, events, &opts.policy, opts.care_window_ms);
        if let Some(timing) = voice::read(&input.dir)? {
            stream.apply_voice(&timing);
        }
        let recording = footage::read_recording(&input.dir);
        let audio = audio_context(recording.as_ref(), opts.ffmpeg.as_deref());
        let re = redactor(&stream.viewer_names);
        let (kept, markers, hook_state) = rank(
            &stream,
            &input.run_id,
            &input.host,
            engine,
            opts,
            re.as_ref(),
            &audio,
        )
        .await;
        let run = RunCtx {
            input,
            recording: recording.as_ref(),
            stream_db: audio.as_ref().ok().map(|a| a.stream_db),
        };
        let now = now_ms();
        let mut drafted = Vec::new();
        for (k, p) in kept.iter().enumerate() {
            let (c, input_json) = candidate(
                &stream,
                &run,
                p,
                k as u32 + 1,
                &markers,
                &opts.policy,
                re.as_ref(),
                now,
            );
            let exclusions = gate::exclusions(&stream, &p.cut, &opts.policy);
            drafted.push((c, input_json, exclusions));
        }
        report.runs.push(ClipScanRun {
            run_id: input.run_id.clone(),
            candidates: drafted.len() as u32,
            hook: hook_state,
            markers_admitted: markers.iter().filter(|m| m.admitted).count() as u32,
            markers_capped: markers.iter().filter(|m| !m.admitted).count() as u32,
        });

        let _lock = store.lock()?;
        let previous: Vec<ClipCandidate> = store
            .all()?
            .into_iter()
            .filter(|c| {
                c.source.run_id == input.run_id && c.source.tournament_id == input.tournament_id
            })
            .collect();
        let owner_acted = |c: &ClipCandidate| {
            c.approval.is_some()
                || c.rejection.is_some()
                || is_recut(c)
                || !matches!(
                    c.status,
                    ClipStatus::RenderedDraft | ClipStatus::PendingOwnerGate
                )
        };
        let keep_ids: HashSet<String> = previous
            .iter()
            .filter(|c| owner_acted(c))
            .map(|c| c.id.clone())
            .collect();
        let feedback: std::collections::HashMap<&str, &shadowrunr_types::clips::ClipFeedback> =
            previous
                .iter()
                .filter_map(|c| c.feedback.as_ref().map(|f| (c.id.as_str(), f)))
                .collect();
        for old in previous.iter().filter(|c| !owner_acted(c)) {
            let dir = store.dir(&old.id);
            std::fs::remove_dir_all(&dir)
                .map_err(|e| crate::ClipError::Io(format!("{}: {e}", dir.display())))?;
            report.replaced += 1;
        }
        for (mut c, input_json, exclusions) in drafted {
            if keep_ids.contains(&c.id) {
                continue;
            }
            c.feedback = feedback.get(c.id.as_str()).map(|f| (*f).clone());
            draft(
                store,
                &mut c,
                &input_json,
                exclusions,
                "render",
                "poster frame, caption tracks and stage input".into(),
            )?;
            report.written += 1;
            fresh.push(c);
        }
        report.kept += keep_ids.len() as u32;
    }
    fresh.sort_by(|a, b| b.score.total_cmp(&a.score));
    report.top = fresh.iter().take(10).map(store::summary).collect();
    Ok(report)
}

/// Write a drafted candidate: its draft files, the gate over `exclusions` plus the render
/// checks, and its first two history entries (the draft, by `by` with `note`, then the gate).
/// The caller holds the store lock.
fn draft(
    store: &ClipStore,
    c: &mut ClipCandidate,
    input_json: &ClipInput,
    exclusions: Vec<shadowrunr_types::clips::GateCheck>,
    by: &str,
    note: String,
) -> Result<()> {
    let dir = store.dir(&c.id);
    let pending = if c.recording.is_some() {
        "not cut yet: `shadowrunr clips render <id>` cuts the recording"
    } else {
        "not recorded yet: `shadowrunr clips render <id>` records the stage when the tools exist"
    };
    let r = render::write_draft(&dir, c, input_json, pending)?;
    c.history.push(ClipTransition {
        status: ClipStatus::RenderedDraft,
        at_ms: r.rendered_ms,
        by: by.into(),
        note: Some(note),
    });
    c.render = Some(r);
    regate(store, c, exclusions);
    c.status = ClipStatus::PendingOwnerGate;
    c.history.push(ClipTransition {
        status: ClipStatus::PendingOwnerGate,
        at_ms: c.gate.evaluated_ms,
        by: "gate".into(),
        note: Some(if c.gate.held {
            format!("held: {}", c.gate.hold_reasons.len())
        } else {
            "every check passed; waits for the owner's signature".into()
        }),
    });
    store.save(c)
}

/// Who a recut's first history entry is by: the mark a rescan keeps it by.
pub const RECUT_BY: &str = "recut";

/// The owner made this candidate with `clips recut`: a rescan keeps it.
pub fn is_recut(c: &ClipCandidate) -> bool {
    c.history.first().is_some_and(|h| h.by == RECUT_BY)
}

/// Re-cut candidate `id` by whole lines (`clips recut`): its first line moves `open` lines
/// (negative is earlier) and its last line `close` lines, a viewer's question travelling with
/// the line that answers it. The result is a NEW candidate under its own id, built from the
/// run's transcript (and voice log) read again: fresh captions and redaction, fresh evidence,
/// and the exclusions recomputed over the whole stream, then gated from the first check at
/// `pending_owner_gate`. Its first history entry names the parent, which is left untouched.
/// The hook read and the loudness are the parent's, carried with a note (they were measured
/// on the parent's window). Refused, nothing written, when the window would drop the payoff
/// or its setup or run over its segment's band ([`window::cut_lines`]), or when another
/// candidate already holds those lines.
pub fn recut(
    store: &ClipStore,
    input: &RunInput,
    id: &str,
    open: i32,
    close: i32,
    opts: &ScanOptions,
) -> Result<ClipCandidate> {
    use crate::ClipError;
    let parent = store.load(id)?;
    if parent.source.run_id != input.run_id || parent.source.tournament_id != input.tournament_id {
        return Err(ClipError::Invalid(format!(
            "clip {id} was cut from run {}, not {}",
            parent.source.run_id, input.run_id
        )));
    }
    if open == 0 && close == 0 {
        return Err(ClipError::Invalid(
            "the recut moves no edge: give --open and/or --close as +-lines".into(),
        ));
    }
    let events = transcript::read_events(&input.dir.join("transcript.jsonl"))?;
    let mut stream = Stream::build(&input.run_id, events, &opts.policy, opts.care_window_ms);
    if let Some(timing) = voice::read(&input.dir)? {
        stream.apply_voice(&timing);
    }
    // Whole lines: one transcript line each, a viewer's question with its answer.
    let mut units: Vec<(u32, usize, usize)> = Vec::new();
    for (i, l) in stream.lines.iter().enumerate() {
        match units.last_mut() {
            Some(u) if u.0 == l.line_no => u.2 = i,
            _ => units.push((l.line_no, i, i)),
        }
    }
    let unit_of = |line_no: u32| {
        units.iter().position(|u| u.0 == line_no).ok_or_else(|| {
            ClipError::Invalid(format!(
                "run {}'s transcript no longer holds line {line_no}",
                input.run_id
            ))
        })
    };
    let (ua, ub) = (
        unit_of(parent.source.line_from)?,
        unit_of(parent.source.line_to)?,
    );
    let moved = |u: usize, by: i32, edge: &str| -> Result<usize> {
        let to = u as i64 + by as i64;
        if to < 0 || to >= units.len() as i64 {
            return Err(ClipError::Invalid(format!(
                "the {edge} edge would move {by:+} lines, past the run's {} spoken line(s)",
                units.len()
            )));
        }
        Ok(to as usize)
    };
    let (na, nb) = (moved(ua, open, "opening")?, moved(ub, close, "closing")?);
    if na > nb {
        return Err(ClipError::Invalid("the recut would hold no line".into()));
    }

    let beat = stream
        .beats
        .iter()
        .position(|b| b.ts_ms == parent.window.anchor_ms)
        .ok_or_else(|| {
            ClipError::Invalid(format!(
                "run {}'s transcript no longer holds the beat at {}",
                input.run_id,
                clock(parent.window.anchor_ms)
            ))
        })?;
    let markers = signals::markers(&stream);
    let crowd = signals::crowd(&stream);
    let marker = signals::beat_marker(&stream, beat, &markers);
    let payoff = window::payoff_line(
        &stream,
        beat,
        marker.map(|i| (&markers[i].kind, markers[i].label.as_str())),
    );
    let segment = stream.segment_at(stream.beats[beat].ts_ms);
    let cut = window::cut_lines(&stream, units[na].1, units[nb].2, payoff, band_for(segment))
        .map_err(|why| ClipError::Invalid(format!("recut of {id}: {why}")))?;

    // The parent's measured reads, carried: they were taken on its window.
    let s = &parent.signals;
    let p = Pre {
        beat,
        payoff,
        marker,
        crowd: crowd[beat],
        segment,
        cut,
        hook: s.hook.map(|h| (h, s.hook_why.clone().unwrap_or_default())),
        audio: match (s.audio, s.audio_db) {
            (Some(a), Some(db)) => Ok((a, db)),
            _ => Err(s
                .unmeasured
                .get("audio")
                .cloned()
                .unwrap_or_else(|| format!("unmeasured on {id}"))),
        },
        score: parent.score,
    };
    let recording = footage::read_recording(&input.dir);
    let run = RunCtx {
        input,
        recording: recording.as_ref(),
        stream_db: s.audio_stream_db,
    };
    let re = redactor(&stream.viewer_names);
    let now = now_ms();
    let (mut c, input_json) = candidate(
        &stream,
        &run,
        &p,
        parent.rank,
        &markers,
        &opts.policy,
        re.as_ref(),
        now,
    );
    if p.hook.is_some() || p.audio.is_ok() {
        c.scoring.provenance.push_str(&format!(
            "; hook and audio carried from {id}'s window, not re-measured on this cut"
        ));
    }
    let exclusions = gate::exclusions(&stream, &p.cut, &opts.policy);
    let note = format!(
        "recut of {id} (open {open:+}, close {close:+}): lines {}-{} -> {}-{}; poster frame, caption tracks and stage input",
        parent.source.line_from, parent.source.line_to, c.source.line_from, c.source.line_to
    );

    let _lock = store.lock()?;
    if c.id == parent.id || store.exists(&c.id) {
        return Err(ClipError::Conflict(format!(
            "clip {} already holds lines {}-{}",
            c.id, c.source.line_from, c.source.line_to
        )));
    }
    draft(store, &mut c, &input_json, exclusions, RECUT_BY, note)?;
    store.load(&c.id)
}

/// Re-render a stored candidate (`clips render`): refused once the owner has approved,
/// rejected or published it; re-gated afterwards. The store lock is held only to snapshot and
/// to commit, never across the render ([`crate::job`]); a failed render keeps the previous one.
pub fn rerender(
    store: &ClipStore,
    id: &str,
    tools: Option<&render::RenderTools>,
) -> Result<ClipCandidate> {
    crate::job::rerender_with(store, id, &crate::job::ToolRenderer(tools))
}
