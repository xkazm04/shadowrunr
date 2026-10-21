//! `transcript.jsonl`: one `StreamEvent` per line. Line numbers are 1-based and are what
//! evidence refs (`<run_id>#<line>`) point at.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use shadowrunr_types::{Speaker, StreamEvent, TsMs};

use crate::sandbox::EventSink;
use crate::{wire, SimError};

/// The one door every transcript line passes through: it assigns the line number, writes the
/// line and, when a sink is attached, hands the event to it at once - before any flush, so a
/// live publisher never waits on the file buffer.
pub struct TranscriptWriter {
    path: PathBuf,
    out: BufWriter<File>,
    events: Vec<StreamEvent>,
    /// Flush after every event, so a reader tailing the file sees each line as it happens.
    flush_each: bool,
    /// Called with each event and its 1-based line, in push order, after the line is written.
    sink: Option<EventSink>,
}

impl TranscriptWriter {
    /// A buffered writer, flushed on [`TranscriptWriter::flush`].
    pub fn create(path: &Path) -> Result<Self, SimError> {
        let f = File::create(path).map_err(|e| SimError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self {
            path: path.to_path_buf(),
            out: BufWriter::new(f),
            events: Vec::new(),
            flush_each: false,
            sink: None,
        })
    }

    /// This writer, handing every pushed event to `sink` (`None`: no observer).
    pub fn with_sink(self, sink: Option<EventSink>) -> Self {
        Self { sink, ..self }
    }

    /// A writer that flushes every event as it is pushed (a live session).
    pub fn create_live(path: &Path) -> Result<Self, SimError> {
        Ok(Self {
            flush_each: true,
            ..Self::create(path)?
        })
    }

    /// Append one event; returns its 1-based line number.
    pub fn push(&mut self, ev: StreamEvent) -> Result<usize, SimError> {
        let line = serde_json::to_string(&ev).map_err(|e| SimError::Io(e.to_string()))?;
        writeln!(self.out, "{line}")
            .map_err(|e| SimError::Io(format!("{}: {e}", self.path.display())))?;
        if self.flush_each {
            self.flush()?;
        }
        self.events.push(ev);
        let line = self.events.len();
        if let (Some(sink), Some(ev)) = (&self.sink, self.events.last()) {
            sink(line as u64, ev);
        }
        Ok(line)
    }

    pub fn push_all(&mut self, evs: impl IntoIterator<Item = StreamEvent>) -> Result<(), SimError> {
        for ev in evs {
            self.push(ev)?;
        }
        Ok(())
    }

    pub fn events(&self) -> &[StreamEvent] {
        &self.events
    }

    pub fn flush(&mut self) -> Result<(), SimError> {
        self.out.flush().map_err(|e| SimError::Io(e.to_string()))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn read(path: &Path) -> Result<Vec<StreamEvent>, SimError> {
    let f = File::open(path).map_err(|e| SimError::Io(format!("{}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(f).lines().enumerate() {
        let line = line.map_err(|e| SimError::Io(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let ev: StreamEvent = serde_json::from_str(&line)
            .map_err(|e| SimError::Io(format!("{}:{}: {e}", path.display(), i + 1)))?;
        out.push(ev);
    }
    Ok(out)
}

pub fn ts(ev: &StreamEvent) -> TsMs {
    match ev {
        StreamEvent::StreamStart { ts_ms, .. }
        | StreamEvent::SegmentStart { ts_ms, .. }
        | StreamEvent::SegmentEnd { ts_ms, .. }
        | StreamEvent::CaseOpen { ts_ms, .. }
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
        | StreamEvent::StreamEnd { ts_ms, .. } => *ts_ms,
    }
}

/// Restamp an event (its `ts_ms`; a chat event's message too).
pub fn set_ts(ev: &mut StreamEvent, ts: TsMs) {
    match ev {
        StreamEvent::StreamStart { ts_ms, .. }
        | StreamEvent::SegmentStart { ts_ms, .. }
        | StreamEvent::SegmentEnd { ts_ms, .. }
        | StreamEvent::CaseOpen { ts_ms, .. }
        | StreamEvent::RecordShown { ts_ms, .. }
        | StreamEvent::LinkPinned { ts_ms, .. }
        | StreamEvent::Picked { ts_ms, .. }
        | StreamEvent::Say { ts_ms, .. }
        | StreamEvent::Op { ts_ms, .. }
        | StreamEvent::Vote { ts_ms, .. }
        | StreamEvent::Moderation { ts_ms, .. }
        | StreamEvent::Handover { ts_ms, .. }
        | StreamEvent::Job { ts_ms, .. }
        | StreamEvent::Health { ts_ms, .. }
        | StreamEvent::BeatPlan { ts_ms, .. }
        | StreamEvent::StreamEnd { ts_ms, .. } => *ts_ms = ts,
        StreamEvent::Chat { ts_ms, msg } => {
            *ts_ms = ts;
            msg.ts_ms = ts;
        }
    }
}

pub fn clock(ts_ms: TsMs) -> String {
    let s = ts_ms.max(0) / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// The content of one event as the judge sees it (without the line number and clock).
pub fn body(ev: &StreamEvent) -> String {
    match ev {
        StreamEvent::StreamStart {
            character,
            tone_variant,
            core_tier,
            audience_size,
            ..
        } => format!(
            "stream_start character={character} tone={tone_variant} core_tier={} audience={audience_size}",
            wire(core_tier)
        ),
        StreamEvent::SegmentStart { segment, .. } => format!("segment_start {}", wire(segment)),
        StreamEvent::SegmentEnd { segment, .. } => format!("segment_end {}", wire(segment)),
        StreamEvent::CaseOpen { case, .. } => format!(
            "case_open {} \"{}\" hexes={} fictional={}",
            case.id,
            case.title,
            case.hexes.len(),
            case.fictional
        ),
        // The summary is what a pinned string citing the record must match (the judge's
        // `role:strings-match-their-records`).
        StreamEvent::RecordShown { record, .. } => format!(
            "record_shown {} [{}] \"{}\" hex={}: {}",
            record.id,
            wire(&record.grade),
            record.title,
            record.hex,
            record.summary
        ),
        StreamEvent::LinkPinned {
            from, to, rel, record_id, grade, ..
        } => format!(
            "link_pinned {from} -{}-> {to} citing {record_id} [{}]",
            wire(rel),
            wire(grade)
        ),
        StreamEvent::Chat { msg, .. } => format!(
            "chat {} ({}{}) [{}]: {}",
            msg.viewer.name,
            wire(&msg.viewer.rank),
            if msg.viewer.is_mod { ", mod" } else { "" },
            msg.id,
            msg.text
        ),
        StreamEvent::Picked { msg_ids, reason, .. } => {
            format!("picked [{}]: {reason}", msg_ids.join(", "))
        }
        StreamEvent::Say {
            speaker, foil, text, ..
        } => match speaker {
            Speaker::Host => format!("say host: {text}"),
            Speaker::Foil => format!("say foil {}: {text}", foil.as_deref().unwrap_or("?")),
        },
        StreamEvent::Op { op, .. } => {
            format!("op {}", serde_json::to_string(op).unwrap_or_default())
        }
        StreamEvent::Vote {
            poll_id,
            tallies,
            closed,
            ..
        } => {
            let t: Vec<String> = tallies
                .iter()
                .map(|t| format!("{}={} voters/{:.1} weight", t.option, t.voters, t.weight))
                .collect();
            format!(
                "vote {poll_id} {}: {}",
                if *closed { "CLOSED" } else { "open" },
                t.join("; ")
            )
        }
        StreamEvent::Moderation {
            stage,
            rule,
            msg_id,
            action,
            ..
        } => format!(
            "moderation {} rule={rule} action={action}{}",
            wire(stage),
            msg_id
                .as_deref()
                .map(|m| format!(" msg={m}"))
                .unwrap_or_default()
        ),
        StreamEvent::Handover {
            from, to, reason, ..
        } => format!("handover {} -> {}: {reason}", wire(from), wire(to)),
        StreamEvent::BeatPlan { mode, bit_id, picks, reason, .. } => format!(
            "beat_plan {}{} picks={picks}: {reason}",
            wire(mode),
            bit_id.as_deref().map(|b| format!(" bit={b}")).unwrap_or_default()
        ),
        StreamEvent::Health { state, detail, .. } => format!(
            "health {}{}",
            wire(state),
            detail
                .as_deref()
                .map(|d| format!(": {d}"))
                .unwrap_or_default()
        ),
        StreamEvent::Job {
            job_id,
            status,
            summary,
            ..
        } => format!(
            "job {job_id} {status}{}",
            summary.as_deref().map(|s| format!(": {s}")).unwrap_or_default()
        ),
        StreamEvent::StreamEnd { reason, .. } => format!("stream_end {}", wire(reason)),
    }
}

pub fn render_line(line: usize, ev: &StreamEvent) -> String {
    format!("L{line} [{}] {}", clock(ts(ev)), body(ev))
}

/// Normalise for quote matching: case, whitespace, typographic quotes and ellipses.
pub fn normalize(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201B}' | '`' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            '\u{2013}' | '\u{2014}' => '-',
            c => c,
        })
        .collect::<String>()
        .replace('\u{2026}', "...");
    mapped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteCheck {
    Verified,
    NoSuchLine,
    EmptyQuote,
    NotFound,
}

/// Minimum normalised quote length: shorter quotes match almost anything.
pub const MIN_QUOTE_CHARS: usize = 4;

/// Check that `quote` occurs verbatim (after normalisation) on transcript line `line`.
pub fn verify_quote(events: &[StreamEvent], line: usize, quote: &str) -> QuoteCheck {
    let Some(ev) = line.checked_sub(1).and_then(|i| events.get(i)) else {
        return QuoteCheck::NoSuchLine;
    };
    let q = normalize(quote.trim().trim_matches(|c| c == '"' || c == '\''));
    let q = q
        .trim_end_matches("...")
        .trim_start_matches("...")
        .trim()
        .to_string();
    if q.chars().count() < MIN_QUOTE_CHARS {
        return QuoteCheck::EmptyQuote;
    }
    if normalize(&body(ev)).contains(&q) {
        QuoteCheck::Verified
    } else {
        QuoteCheck::NotFound
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::{ChatMsg, Platform, Rank, Viewer};

    pub(crate) fn chat(ts: TsMs, id: &str, name: &str, text: &str) -> StreamEvent {
        StreamEvent::Chat {
            ts_ms: ts,
            msg: ChatMsg {
                id: id.into(),
                ts_ms: ts,
                viewer: Viewer {
                    id: format!("v-{name}"),
                    name: name.into(),
                    platform: Platform::Sim,
                    rank: Rank::Cultist,
                    sub_tier: None,
                    months: 0,
                    gifted: 0,
                    is_mod: false,
                },
                text: text.into(),
                command: None,
            },
        }
    }

    fn say(ts: TsMs, text: &str) -> StreamEvent {
        StreamEvent::Say {
            ts_ms: ts,
            speaker: Speaker::Host,
            foil: None,
            text: text.into(),
            replying_to: vec![],
            first_text_ms: None,
        }
    }

    #[test]
    fn quote_verification() {
        let evs = vec![
            chat(1000, "m1", "GloomToad", "say the forbidden spell word"),
            say(
                5000,
                "The Lord forbids such crude tools\u{2026} next scheme!",
            ),
        ];
        assert_eq!(
            verify_quote(&evs, 2, "the Lord forbids such crude tools..."),
            QuoteCheck::Verified
        );
        assert_eq!(
            verify_quote(&evs, 2, "\"The   Lord forbids\""),
            QuoteCheck::Verified
        );
        // A fabricated quote does not verify.
        assert_eq!(
            verify_quote(&evs, 2, "I am a real human being"),
            QuoteCheck::NotFound
        );
        // The right words on the wrong line do not verify either.
        assert_eq!(
            verify_quote(&evs, 1, "the Lord forbids"),
            QuoteCheck::NotFound
        );
        assert_eq!(
            verify_quote(&evs, 3, "the Lord forbids"),
            QuoteCheck::NoSuchLine
        );
        assert_eq!(
            verify_quote(&evs, 0, "the Lord forbids"),
            QuoteCheck::NoSuchLine
        );
        assert_eq!(verify_quote(&evs, 2, "  "), QuoteCheck::EmptyQuote);
        assert_eq!(verify_quote(&evs, 2, "..."), QuoteCheck::EmptyQuote);
    }

    #[test]
    fn write_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        let mut w = TranscriptWriter::create(&p).unwrap();
        assert_eq!(w.push(chat(0, "m1", "A", "hi")).unwrap(), 1);
        assert_eq!(w.push(say(5000, "hello minion")).unwrap(), 2);
        w.flush().unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.len(), 2);
        assert!(render_line(2, &back[1]).starts_with("L2 [00:05] say host: hello minion"));
    }

    #[test]
    fn a_live_writer_flushes_every_event_and_a_buffered_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("live.jsonl");
        let mut w = TranscriptWriter::create_live(&live).unwrap();
        w.push(chat(0, "m1", "A", "hi")).unwrap();
        assert_eq!(read(&live).unwrap().len(), 1);
        w.push(say(5000, "hello minion")).unwrap();
        assert_eq!(read(&live).unwrap().len(), 2);
        let buffered = dir.path().join("buffered.jsonl");
        let mut b = TranscriptWriter::create(&buffered).unwrap();
        b.push(chat(0, "m1", "A", "hi")).unwrap();
        assert_eq!(read(&buffered).unwrap().len(), 0);
    }

    #[test]
    fn a_sink_hears_every_push_in_order_before_any_flush() {
        use std::sync::{Arc, Mutex};
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        let heard: Arc<Mutex<Vec<(u64, StreamEvent)>>> = Arc::default();
        let sink: crate::sandbox::EventSink = {
            let heard = heard.clone();
            Arc::new(move |seq, ev: &StreamEvent| heard.lock().unwrap().push((seq, ev.clone())))
        };
        let mut w = TranscriptWriter::create(&p).unwrap().with_sink(Some(sink));
        let evs: Vec<StreamEvent> = (0..5)
            .map(|i| chat(i * 1000, &format!("m{i}"), "A", &format!("line {i}")))
            .collect();
        for ev in &evs {
            w.push(ev.clone()).unwrap();
        }
        // Buffered: nothing on disk yet, yet the sink heard all five, numbered by line.
        assert_eq!(read(&p).unwrap().len(), 0, "no flush happened");
        let heard = heard.lock().unwrap().clone();
        let seqs: Vec<u64> = heard.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, [1, 2, 3, 4, 5]);
        let got: Vec<StreamEvent> = heard.into_iter().map(|(_, e)| e).collect();
        assert_eq!(
            serde_json::to_value(&got).unwrap(),
            serde_json::to_value(&evs).unwrap()
        );
        // No sink is the plain buffered writer.
        let mut plain = TranscriptWriter::create(&dir.path().join("p.jsonl"))
            .unwrap()
            .with_sink(None);
        assert_eq!(plain.push(evs[0].clone()).unwrap(), 1);
    }

    #[test]
    fn set_ts_restamps_a_chat_message_with_its_event() {
        let mut ev = chat(1000, "m1", "A", "hi");
        set_ts(&mut ev, 4000);
        let StreamEvent::Chat { ts_ms, msg } = &ev else {
            panic!("{ev:?}");
        };
        assert_eq!((*ts_ms, msg.ts_ms), (4000, 4000));
        let mut s = say(5000, "hello");
        set_ts(&mut s, 6000);
        assert_eq!(ts(&s), 6000);
    }

    #[test]
    fn case_open_passes_through_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.jsonl");
        let case = shadowrunr_segments::demo_case();
        let mut w = TranscriptWriter::create(&p).unwrap();
        w.push(StreamEvent::CaseOpen {
            ts_ms: 0,
            case: case.clone(),
        })
        .unwrap();
        w.flush().unwrap();
        let back = read(&p).unwrap();
        let [StreamEvent::CaseOpen {
            ts_ms: 0,
            case: got,
        }] = back.as_slice()
        else {
            panic!("expected one case_open, got {back:?}");
        };
        assert_eq!(
            serde_json::to_value(got).unwrap(),
            serde_json::to_value(&case).unwrap()
        );
        assert!(got.fictional);
        let line = render_line(1, &back[0]);
        assert!(
            line.starts_with(&format!("L1 [00:00] case_open {}", case.id)),
            "{line}"
        );
        assert!(line.contains("fictional=true"), "{line}");
    }
}
