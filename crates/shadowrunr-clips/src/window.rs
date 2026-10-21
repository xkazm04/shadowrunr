//! Peak minus pre-roll: a scored beat becomes a cut on the spoken clock (the voice's real
//! timing when the run has a voice log, else the caption model's estimate).
//!
//! The payoff is the line of the anchor beat that carries the moment ([`payoff_line`]): a
//! simulated beat holds three lines and ~30 s of speech, so the whole beat would not fit a
//! short's band. The cut opens [`PREROLL_MS`] before the payoff,
//! snapped *earlier* to the start of the line that holds that instant (never mid-line), and
//! closes one reaction beat ([`REACTION_TAIL_MS`]) after the payoff. It is then held inside
//! its segment's band: too short, it takes the following lines (the reaction) and then
//! earlier ones; too long, the pre-roll shrinks line by line but never past the setup, the
//! line the payoff answers. A payoff and setup that cannot fit is flagged for re-authoring
//! rather than cut. The 12 s pre-roll and the bands are the digest's craft defaults, not
//! measurements.
//!
//! On the voice's real timing the edges snap to what is heard ([`edge_start`], [`edge_end`]):
//! the cut opens up to [`VOICE_LEAD_MS`] before its first line, but never into the line heard
//! before it, and its reaction tail stops where the next heard line starts, so no cut opens or
//! closes on half a word.

use shadowrunr_types::clips::MarkerKind;
use shadowrunr_types::SegmentId;

use crate::transcript::{Stream, Who};

/// The digest's pre-roll (§ 7), on the spoken clock.
pub const PREROLL_MS: i64 = 12_000;
/// One beat of reaction after the payoff's last word.
pub const REACTION_TAIL_MS: i64 = 1_500;
/// On real timing, the breath of picture before the first line, when there is silence for it.
pub const VOICE_LEAD_MS: i64 = 400;

/// A line that made sound: a heard host line on real timing (a viewer's question is shown,
/// never spoken; an unvoiced line was never heard).
fn heard(l: &crate::transcript::Line) -> bool {
    !l.unvoiced && !matches!(l.who, Who::Viewer { .. })
}

/// Where a cut whose first line is `first` opens: the line's start, and on real timing up to
/// [`VOICE_LEAD_MS`] earlier but not before the previous heard line has ended.
pub fn edge_start(stream: &Stream, first: usize) -> i64 {
    let lines = &stream.lines;
    let s = lines[first].spoken_start;
    if !stream.voice_timed {
        return s;
    }
    let prev = lines[..first]
        .iter()
        .rev()
        .filter(|l| heard(l))
        .take(3)
        .map(|l| l.spoken_end)
        .max();
    s - prev.map_or(VOICE_LEAD_MS, |e| (s - e).clamp(0, VOICE_LEAD_MS))
}

/// Where a cut whose last line is `last` closes: one reaction beat after the line, and on real
/// timing never past the start of the next heard line.
pub fn edge_end(stream: &Stream, last: usize) -> i64 {
    let lines = &stream.lines;
    let e = lines[last].spoken_end + REACTION_TAIL_MS;
    if !stream.voice_timed {
        return e;
    }
    match lines[last + 1..].iter().find(|l| heard(l)) {
        Some(n) => e.min(n.spoken_start.max(lines[last].spoken_end)),
        None => e,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Band {
    pub min_ms: i64,
    pub max_ms: i64,
    pub style: &'static str,
}

/// SEG-01 split-stage 25-35 s; SEG-02A 45-60 s (the cut must also show its source and
/// grade); anything else 15-60 s.
pub fn band_for(seg: Option<SegmentId>) -> Band {
    match seg {
        Some(SegmentId::Seg01ImprovisedRoom) => Band {
            min_ms: 25_000,
            max_ms: 35_000,
            style: "seg01-split-stage",
        },
        Some(SegmentId::Seg02aHexmap) => Band {
            min_ms: 45_000,
            max_ms: 60_000,
            style: "seg02a-alleged",
        },
        _ => Band {
            min_ms: 15_000,
            max_ms: 60_000,
            style: "default",
        },
    }
}

/// A cut: lines `first..=last` of the stream, played from `spoken_start` to `spoken_end`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub first: usize,
    pub last: usize,
    pub spoken_start: i64,
    pub spoken_end: i64,
    /// How far before the payoff the cut opens.
    pub preroll_ms: i64,
    pub needs_reauthoring: bool,
    pub note: Option<String>,
}

impl Cut {
    pub fn duration_ms(&self) -> i64 {
        self.spoken_end - self.spoken_start
    }
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
        .collect()
}

/// The line of `beat` that carries the moment: for a marker, the host line sharing most words
/// with its label (a hex reveal also matches the hex id); for a foil roast, the host's first
/// answer after the foil's line; otherwise (a crowd peak) the beat's last host line, the one
/// the crowd reacted to.
pub fn payoff_line(stream: &Stream, beat: usize, marker: Option<(&MarkerKind, &str)>) -> usize {
    let b = &stream.beats[beat];
    let host: Vec<usize> = (b.first..=b.last)
        .filter(|&i| !matches!(stream.lines[i].who, Who::Viewer { .. }))
        .collect();
    let Some(&last) = host.last() else {
        return b.last;
    };
    match marker {
        Some((MarkerKind::FoilRoast, _)) => host
            .iter()
            .copied()
            .skip_while(|&i| !matches!(stream.lines[i].who, Who::Foil(_)))
            .find(|&i| matches!(stream.lines[i].who, Who::Host))
            .or_else(|| {
                host.iter()
                    .copied()
                    .find(|&i| matches!(stream.lines[i].who, Who::Host))
            })
            .unwrap_or(last),
        Some((_, label)) => {
            let want = words(label);
            let mut best = (0usize, last);
            for &i in host.iter().rev() {
                let have = words(&stream.lines[i].text);
                let n = want.iter().filter(|w| have.contains(w)).count();
                if n > best.0 {
                    best = (n, i);
                }
            }
            best.1
        }
        None => last,
    }
}

/// The setup the payoff cannot lose: its question when the beat opens on one, else the line
/// before it.
pub fn setup_line(stream: &Stream, payoff: usize) -> usize {
    if matches!(stream.lines[payoff].who, Who::Viewer { .. }) || payoff == 0 {
        payoff
    } else {
        payoff - 1
    }
}

/// The cut around the payoff line `payoff` (an index into `stream.lines`).
pub fn cut(stream: &Stream, payoff: usize, band: Band) -> Cut {
    let lines = &stream.lines;
    let (pf, pl) = (payoff, payoff);
    let payoff_start = lines[pf].spoken_start;
    let min_first = setup_line(stream, pf);
    let target = payoff_start - PREROLL_MS;
    // The last line that starts at or before the target: the edge only ever moves earlier.
    // A scan, not a bisection: on real timing an unheard line keeps its stream-clock time, so
    // starts are not guaranteed to be in order.
    let mut first = (0..=pf)
        .rev()
        .find(|&j| lines[j].spoken_start <= target)
        .unwrap_or(0)
        .min(min_first);
    let mut last = pl;
    let end_of = |last: usize| edge_end(stream, last);
    let start_of = |first: usize| edge_start(stream, first);
    let mut start = start_of(first);
    let mut end = end_of(last);
    let mut note = None;
    let mut needs_reauthoring = false;

    if end - start > band.max_ms {
        while first < min_first && end - start > band.max_ms {
            first += 1;
            start = start_of(first);
        }
        if end - start > band.max_ms {
            needs_reauthoring = true;
            note = Some(format!(
                "setup and payoff run {:.1} s, over the {} s band: re-author, do not cut the setup",
                (end - start) as f64 / 1000.0,
                band.max_ms / 1000
            ));
        }
    }
    if end - start < band.min_ms {
        // The reaction first, then more setup, in whole lines.
        while last + 1 < lines.len()
            && end - start < band.min_ms
            && end_of(last + 1) - start <= band.max_ms
        {
            last += 1;
            end = end_of(last);
        }
        while first > 0 && end - start < band.min_ms && end - start_of(first - 1) <= band.max_ms {
            first -= 1;
            start = start_of(first);
        }
        if end - start < band.min_ms {
            end = start + band.min_ms;
            note = Some(format!(
                "held on the last line to the band's {} s minimum",
                band.min_ms / 1000
            ));
        }
    }
    Cut {
        first,
        last,
        spoken_start: start,
        spoken_end: end,
        preroll_ms: payoff_start - start,
        needs_reauthoring,
        note,
    }
}

/// An owner's cut over exactly lines `first..=last` around the payoff line `payoff`, with the
/// same edge snapping as [`cut`]. Refused, with the figure, when it would drop the payoff or
/// its setup ([`setup_line`]) or run over the band: the setup is never cut to meet the band,
/// and a recut is never longer than the band allows. Under the band's minimum it is held on
/// the last line, as [`cut`] holds it.
pub fn cut_lines(
    stream: &Stream,
    first: usize,
    last: usize,
    payoff: usize,
    band: Band,
) -> Result<Cut, String> {
    let lines = &stream.lines;
    if first > last || last >= lines.len() {
        return Err("the window would hold no line".into());
    }
    if last < payoff {
        return Err(format!(
            "the window would close on line {}, before the payoff (line {})",
            lines[last].line_no, lines[payoff].line_no
        ));
    }
    let min_first = setup_line(stream, payoff);
    if first > min_first {
        return Err(format!(
            "the window would open on line {}, after the payoff's setup (line {}): the setup is never cut",
            lines[first].line_no, lines[min_first].line_no
        ));
    }
    let start = edge_start(stream, first);
    let mut end = edge_end(stream, last);
    if end - start > band.max_ms {
        return Err(format!(
            "the window would run {:.1} s, over the {} s band",
            (end - start) as f64 / 1000.0,
            band.max_ms / 1000
        ));
    }
    let mut note = None;
    if end - start < band.min_ms {
        end = start + band.min_ms;
        note = Some(format!(
            "held on the last line to the band's {} s minimum",
            band.min_ms / 1000
        ));
    }
    Ok(Cut {
        first,
        last,
        spoken_start: start,
        spoken_end: end,
        preroll_ms: lines[payoff].spoken_start - start,
        needs_reauthoring: false,
        note,
    })
}
