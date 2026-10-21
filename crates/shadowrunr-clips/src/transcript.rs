//! A transcript read for clipping: its events with their line numbers, the spoken lines in
//! the order and on the clock they would play aloud, the beats those lines belong to, and
//! the stream state the gate needs (who chatted, who is in care, what moderation did).
//!
//! **Two clocks.** `ts_ms` is the stream clock. A simulated beat is a 5 s tick that can hold
//! three lines of a dozen words each, far more speech than its tick, so the draft plays on a
//! *spoken* clock: each line starts at its beat or when the previous line has finished,
//! whichever is later, and lasts as long as the stage's caption model says it takes to say
//! (`packages/stage-kit/src/timing.ts`, ported here so the caption files and the recorded
//! stage agree).
//!
//! **Real timing.** When the run has a `voice.jsonl`, [`Stream::apply_voice`] replaces the
//! estimate with what the voice did: a heard line runs from its first `played` to its last
//! part's end, on the stream clock. A host line never heard keeps its stream-clock time and is
//! flagged `unvoiced`; a viewer's question moves with the line that answers it.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use shadowrunr_brain::moderation::{Policy, TermAction};
use shadowrunr_types::{Case, ModStage, SegmentId, Speaker, StreamEvent, TsMs};

use crate::voice::VoiceTiming;
use crate::{ClipError, Result};

/// Stage-kit `wordTimings` constants (`packages/stage-kit/src/timing.ts`).
const BASE_WORD_MS: i64 = 230;
const PER_CHAR_MS: i64 = 24;
const PAUSE_COMMA_MS: i64 = 170;
const PAUSE_STOP_MS: i64 = 360;
const PAUSE_ELLIPSIS_MS: i64 = 560;
/// The breath between two spoken lines.
pub const BREATH_MS: i64 = 250;
/// A viewer's question shown on screen stays at least this long.
const MIN_CUE_MS: i64 = 1_500;

/// How long a line takes to say, by the stage's caption model.
pub fn spoken_ms(text: &str) -> i64 {
    let mut t = 0;
    for word in text.split_whitespace() {
        let letters = word.chars().filter(|c| c.is_alphanumeric()).count() as i64;
        t += BASE_WORD_MS + letters.min(14) * PER_CHAR_MS + pause_after(word);
    }
    t
}

fn pause_after(word: &str) -> i64 {
    let w = word.trim_end_matches(['"', '\'', ')', ']']);
    if w.ends_with("...") || w.ends_with('…') {
        PAUSE_ELLIPSIS_MS
    } else if w.ends_with(['.', '!', '?']) {
        PAUSE_STOP_MS
    } else if w.ends_with([',', ';', ':', '—', '–']) || word == "—" || word == "–" {
        PAUSE_COMMA_MS
    } else {
        0
    }
}

/// Who a spoken line belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Who {
    Malakor,
    Foil(String),
    /// A viewer's message a host line answers, shown before the answer (its question).
    Viewer {
        msg_id: String,
    },
}

impl Who {
    pub fn label(&self) -> String {
        match self {
            Who::Malakor => "malakor".into(),
            Who::Foil(f) => f.clone(),
            Who::Viewer { .. } => "viewer".into(),
        }
    }
}

/// One line on the spoken clock.
#[derive(Debug, Clone)]
pub struct Line {
    /// Index into [`Stream::events`].
    pub event: usize,
    pub line_no: u32,
    pub ts_ms: TsMs,
    pub who: Who,
    pub text: String,
    pub replying_to: Vec<String>,
    pub spoken_start: i64,
    pub spoken_end: i64,
    /// Real timing applied and this host line was never heard.
    pub unvoiced: bool,
}

/// The lines spoken at one stream-clock tick (a viewer cue belongs to the beat it is
/// answered in).
#[derive(Debug, Clone)]
pub struct Beat {
    pub ts_ms: TsMs,
    /// Indices into [`Stream::lines`], contiguous.
    pub first: usize,
    pub last: usize,
}

#[derive(Debug, Clone)]
pub struct ChatInfo {
    pub ts_ms: TsMs,
    pub viewer_id: String,
    pub name: String,
    pub text: String,
    pub is_command: bool,
}

#[derive(Debug, Clone)]
pub struct ModInfo {
    pub ts_ms: TsMs,
    pub stage: ModStage,
    pub rule: String,
    pub msg_id: Option<String>,
    pub action: String,
}

/// A viewer in care over `[from_ms, until_ms)` after a self-harm disclosure.
#[derive(Debug, Clone)]
pub struct CareSpan {
    pub viewer_id: String,
    pub name: String,
    pub msg_id: String,
    pub from_ms: TsMs,
    pub until_ms: TsMs,
}

#[derive(Debug, Clone)]
pub struct Stream {
    pub stream_id: String,
    pub events: Vec<(u32, StreamEvent)>,
    pub lines: Vec<Line>,
    pub beats: Vec<Beat>,
    pub chats: HashMap<String, ChatInfo>,
    pub mods: Vec<ModInfo>,
    /// Every viewer handle that chatted (for caption redaction).
    pub viewer_names: Vec<String>,
    /// `(from, segment)`, in order.
    pub segments: Vec<(TsMs, SegmentId)>,
    /// `(opened, case)`, in order.
    pub cases: Vec<(TsMs, Case)>,
    pub care: Vec<CareSpan>,
    /// Messages moderation tagged as a prompt injection.
    pub injections: HashSet<String>,
    pub end_ms: TsMs,
    /// The spoken-clock time of each event (index-aligned with `events`).
    pub spoken_at: Vec<i64>,
    /// The spoken clock is the voice's real timing ([`Stream::apply_voice`]).
    pub voice_timed: bool,
}

/// Read `transcript.jsonl`. A torn last line (a run still being written) is skipped; a bad
/// line anywhere else is an error naming the line.
pub fn read_events(path: &Path) -> Result<Vec<(u32, StreamEvent)>> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ClipError::NotFound(format!("{}: {e}", path.display())))?;
    parse_events(&text, &path.display().to_string())
}

pub fn parse_events(text: &str, label: &str) -> Result<Vec<(u32, StreamEvent)>> {
    let raw: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in raw.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<StreamEvent>(line) {
            Ok(ev) => out.push((i as u32 + 1, ev)),
            Err(_) if i + 1 == raw.len() && !text.ends_with('\n') => break,
            Err(e) => return Err(ClipError::Io(format!("{label}:{}: {e}", i + 1))),
        }
    }
    Ok(out)
}

pub fn ts_of(ev: &StreamEvent) -> TsMs {
    match ev {
        StreamEvent::StreamStart { ts_ms, .. }
        | StreamEvent::SegmentStart { ts_ms, .. }
        | StreamEvent::SegmentEnd { ts_ms, .. }
        | StreamEvent::CaseOpen { ts_ms, .. }
        | StreamEvent::RecordShown { ts_ms, .. }
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
        | StreamEvent::StreamEnd { ts_ms, .. } => *ts_ms,
    }
}

impl Stream {
    /// Build the clipping view of a transcript. `policy` decides which viewer questions may
    /// be shown at all (a blocked or bleeped message never becomes a cue); `care_window_ms` is
    /// the brain's care window.
    pub fn build(
        stream_id: &str,
        events: Vec<(u32, StreamEvent)>,
        policy: &Policy,
        care_window_ms: i64,
    ) -> Self {
        let mut chats = HashMap::new();
        let mut mods = Vec::new();
        let mut segments = Vec::new();
        let mut cases = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut stream_id = stream_id.to_string();
        let mut end_ms = 0;
        for (_, ev) in &events {
            end_ms = end_ms.max(ts_of(ev));
            match ev {
                StreamEvent::StreamStart { stream_id: s, .. } => stream_id = s.clone(),
                StreamEvent::Chat { msg, .. } => {
                    if !names.contains(&msg.viewer.name) {
                        names.push(msg.viewer.name.clone());
                    }
                    chats.insert(
                        msg.id.clone(),
                        ChatInfo {
                            ts_ms: msg.ts_ms,
                            viewer_id: msg.viewer.id.clone(),
                            name: msg.viewer.name.clone(),
                            text: msg.text.clone(),
                            is_command: msg.command.is_some(),
                        },
                    );
                }
                StreamEvent::Moderation {
                    ts_ms,
                    stage,
                    rule,
                    msg_id,
                    action,
                } => mods.push(ModInfo {
                    ts_ms: *ts_ms,
                    stage: *stage,
                    rule: rule.clone(),
                    msg_id: msg_id.clone(),
                    action: action.clone(),
                }),
                StreamEvent::SegmentStart { ts_ms, segment } => segments.push((*ts_ms, *segment)),
                StreamEvent::Handover { ts_ms, to, .. } => segments.push((*ts_ms, *to)),
                StreamEvent::CaseOpen { ts_ms, case } => cases.push((*ts_ms, case.clone())),
                _ => {}
            }
        }
        let injections: HashSet<String> = mods
            .iter()
            .filter(|m| m.rule == "prompt_injection")
            .filter_map(|m| m.msg_id.clone())
            .collect();
        let care = care_spans(&mods, &chats, care_window_ms);
        let disclosures: HashSet<&str> = care.iter().map(|c| c.msg_id.as_str()).collect();
        let dropped: HashSet<&str> = mods
            .iter()
            .filter(|m| m.stage == ModStage::Input && m.action == "dropped" && m.rule != "rate_cap")
            .filter_map(|m| m.msg_id.as_deref())
            .collect();
        let showable = |id: &str, c: &ChatInfo| {
            !c.is_command
                && !injections.contains(id)
                && !disclosures.contains(id)
                && !dropped.contains(id)
                && policy.self_harm_disclosure(&c.text).is_none()
                && !matches!(
                    policy.strictest(&c.text).map(|r| r.action),
                    Some(TermAction::Block | TermAction::Bleep)
                )
        };

        // Spoken lines and beats.
        let mut lines: Vec<Line> = Vec::new();
        let mut beats: Vec<Beat> = Vec::new();
        let mut cued: HashSet<String> = HashSet::new();
        let mut cursor: i64 = 0;
        for (i, (line_no, ev)) in events.iter().enumerate() {
            let StreamEvent::Say {
                ts_ms,
                speaker,
                foil,
                text,
                replying_to,
                ..
            } = ev
            else {
                continue;
            };
            let new_beat = beats.last().is_none_or(|b| b.ts_ms != *ts_ms);
            let mut start = if new_beat { cursor.max(*ts_ms) } else { cursor };
            let first_of_beat = lines.len();
            for id in replying_to {
                let Some(c) = chats.get(id) else { continue };
                if cued.contains(id) || !showable(id, c) {
                    continue;
                }
                cued.insert(id.clone());
                let dur = spoken_ms(&c.text).max(MIN_CUE_MS);
                lines.push(Line {
                    event: i,
                    line_no: *line_no,
                    ts_ms: *ts_ms,
                    who: Who::Viewer { msg_id: id.clone() },
                    text: c.text.clone(),
                    replying_to: Vec::new(),
                    spoken_start: start,
                    spoken_end: start + dur,
                    unvoiced: false,
                });
                start += dur + BREATH_MS;
            }
            let dur = spoken_ms(text).max(1);
            lines.push(Line {
                event: i,
                line_no: *line_no,
                ts_ms: *ts_ms,
                who: match speaker {
                    Speaker::Malakor => Who::Malakor,
                    Speaker::Foil => Who::Foil(foil.clone().unwrap_or_else(|| "foil".into())),
                },
                text: text.clone(),
                replying_to: replying_to.clone(),
                spoken_start: start,
                spoken_end: start + dur,
                unvoiced: false,
            });
            cursor = start + dur + BREATH_MS;
            if new_beat {
                beats.push(Beat {
                    ts_ms: *ts_ms,
                    first: first_of_beat,
                    last: lines.len() - 1,
                });
            } else if let Some(b) = beats.last_mut() {
                b.last = lines.len() - 1;
            }
        }
        let spoken_at = spoken_positions(&events, &lines);
        Stream {
            stream_id,
            events,
            lines,
            beats,
            chats,
            mods,
            viewer_names: names,
            segments,
            cases,
            care,
            injections,
            end_ms,
            spoken_at,
            voice_timed: false,
        }
    }

    /// Re-time every line from the voice log: a heard host line takes its real start and end;
    /// one never heard starts at its stream-clock time, lasts its estimate and is flagged
    /// `unvoiced`; a viewer's question ends a breath before the line that answers it, lasting
    /// its estimate but never reaching back past the line before it.
    pub fn apply_voice(&mut self, timing: &VoiceTiming) {
        // Each line's estimated length, before the re-timing.
        let estimate: Vec<i64> = self
            .lines
            .iter()
            .map(|l| l.spoken_end - l.spoken_start)
            .collect();
        for l in self.lines.iter_mut() {
            if matches!(l.who, Who::Viewer { .. }) {
                continue;
            }
            match timing.heard(u64::from(l.line_no)) {
                Some(h) => {
                    l.spoken_start = h.start_ms;
                    l.spoken_end = h.end_ms;
                    l.unvoiced = false;
                }
                None => {
                    l.spoken_start = l.ts_ms;
                    l.spoken_end = l.ts_ms + spoken_ms(&l.text).max(1);
                    l.unvoiced = true;
                }
            }
        }
        for (i, len) in estimate.into_iter().enumerate() {
            if !matches!(self.lines[i].who, Who::Viewer { .. }) {
                continue;
            }
            let ev = self.lines[i].event;
            let answer = (i + 1..self.lines.len()).find(|&j| {
                self.lines[j].event == ev && !matches!(self.lines[j].who, Who::Viewer { .. })
            });
            if let Some(j) = answer {
                let end = self.lines[j].spoken_start - BREATH_MS;
                let floor = i
                    .checked_sub(1)
                    .map_or(i64::MIN, |p| self.lines[p].spoken_end + BREATH_MS);
                let start = (end - len).max(floor).min(end - MIN_CUE_MS / 2);
                self.lines[i].spoken_start = start;
                self.lines[i].spoken_end = end.max(start + 1);
            }
        }
        self.spoken_at = spoken_positions(&self.events, &self.lines);
        self.voice_timed = true;
    }

    /// The segment running at `ts`.
    pub fn segment_at(&self, ts: TsMs) -> Option<SegmentId> {
        self.segments
            .iter()
            .take_while(|(from, _)| *from <= ts)
            .last()
            .map(|(_, s)| *s)
    }

    /// The case open at `ts` (the latest `case_open` at or before it).
    pub fn case_at(&self, ts: TsMs) -> Option<&Case> {
        self.cases
            .iter()
            .take_while(|(from, _)| *from <= ts)
            .last()
            .map(|(_, c)| c)
    }

    /// The beat at or before `ts`.
    pub fn beat_at_or_before(&self, ts: TsMs) -> Option<usize> {
        let n = self.beats.partition_point(|b| b.ts_ms <= ts);
        n.checked_sub(1)
    }
}

/// When each event happens on the spoken clock: a non-spoken event in a beat lands when the
/// lines before it in that beat have been said; anything else lands when the next line
/// starts.
fn spoken_positions(events: &[(u32, StreamEvent)], lines: &[Line]) -> Vec<i64> {
    let mut out = vec![0; events.len()];
    // Last line (by event index) at or before each event, same tick.
    let mut li = 0;
    let mut prev_end: Option<(TsMs, i64)> = None;
    for (i, (_, ev)) in events.iter().enumerate() {
        while li < lines.len() && lines[li].event < i {
            prev_end = Some((lines[li].ts_ms, lines[li].spoken_end));
            li += 1;
        }
        let ts = ts_of(ev);
        out[i] = match (lines.get(li), prev_end) {
            (Some(l), _) if l.event == i => l.spoken_start,
            (_, Some((pts, end))) if pts == ts => end,
            (Some(next), _) => next.spoken_start,
            (None, Some((_, end))) => end.max(ts),
            (None, None) => ts,
        };
    }
    out
}

/// Care spans from the transcript's `care_state` moderation events (started / restarted /
/// expired), keyed by the disclosure's message.
fn care_spans(
    mods: &[ModInfo],
    chats: &HashMap<String, ChatInfo>,
    window_ms: i64,
) -> Vec<CareSpan> {
    let mut spans: Vec<CareSpan> = Vec::new();
    for m in mods
        .iter()
        .filter(|m| m.rule == shadowrunr_brain::care::CARE_RULE)
    {
        let Some(id) = &m.msg_id else { continue };
        let Some(c) = chats.get(id) else { continue };
        match m.action.as_str() {
            "started" | "restarted" => {
                // A restart moves the window: close any open span of this viewer first.
                for s in spans.iter_mut().filter(|s| s.viewer_id == c.viewer_id) {
                    s.until_ms = s.until_ms.min(m.ts_ms);
                }
                spans.push(CareSpan {
                    viewer_id: c.viewer_id.clone(),
                    name: c.name.clone(),
                    msg_id: id.clone(),
                    from_ms: c.ts_ms.min(m.ts_ms),
                    until_ms: m.ts_ms + window_ms,
                });
            }
            "expired" => {
                for s in spans.iter_mut().filter(|s| s.viewer_id == c.viewer_id) {
                    s.until_ms = s.until_ms.min(m.ts_ms);
                }
            }
            _ => {}
        }
    }
    spans
}
