//! The watched checks: what a viewer of the show sees and hears, beside the checks that read
//! text (contract § sandbox, the watched checks).
//!
//! - `voiced` and `face` read the run's `voice.jsonl`: which of the host's lines were played in
//!   full, and which face the stage drew.
//! - `av_sync` and `dead_air` read the recording (`recording.json` and the file it names) with
//!   ffmpeg's `silencedetect`: speech onsets against the `played` times, and the longest real
//!   silence in the programme audio.
//!
//! A check with nothing to read is `unmeasured`. Whether the watched checks gate the session is
//! the watched-gate rule ([`shadowrunr_types::sandbox::check_gates`]): only a recorded session.
//! The daemon fills them when a session ends (the recording is not in the run dir yet, so the
//! recording checks are unmeasured there); `stream sandbox --record` fills them again once the
//! recording is in, and `stream check` recomputes them offline. Each run rewrites
//! `session.json`, the one report the console reads.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use shadowrunr_types::sandbox::{
    gate_pass, SandboxCheck, SandboxCheckId, SandboxCheckState, SandboxReport,
};
use shadowrunr_types::voice::{VoiceLogEvent, VoiceLogLine};
use shadowrunr_types::{Op, Speaker, StreamEvent, TsMs};

use super::{Recording, META_FILE};

/// `voiced` passes when at least this share of the host's lines played in full. Assumption: the
/// PACE target.
pub const VOICED_MIN_SHARE: f64 = 0.90;
/// `face` passes when the real rig was drawn for at least this share of the session.
/// Assumption.
pub const FACE_MIN_SHARE: f64 = 0.95;
/// Speech onsets: the end of a silence under this level... Measured: REC's sync check found the
/// line starts at -35 dB (sbx-20261021-085027-5m).
pub const ONSET_NOISE_DB: f64 = -35.0;
/// ...that lasted at least this long. Measured, with the level above.
pub const ONSET_MIN_SILENCE_S: f64 = 0.25;
/// An onset matches a `played` time at most this far from it. Assumption: three times the
/// ~200 ms constant REC measured, so a line start never matches the next line's.
pub const ONSET_MATCH_MS: i64 = 600;
/// Head and tail are each at most this many matched line starts.
pub const AV_SYNC_WINDOW: usize = 8;
/// Fewer matched line starts than this leave `av_sync` unmeasured.
pub const AV_SYNC_MIN_MATCHES: usize = 4;
/// Sound earlier than the run's constant by more than this is noticeable. Assumption: the
/// `video-assembly` registry subject (noticeable from ~45 ms early).
pub const AV_SYNC_EARLY_MS: i64 = 45;
/// Sound later than the run's constant by more than this is noticeable. Assumption: the
/// `video-assembly` subject (~100 ms late noticeable; 125 ms the detectability bound).
pub const AV_SYNC_LATE_MS: i64 = 125;
/// The head-to-tail drift of the offset may be at most this. Assumption.
pub const AV_DRIFT_MAX_MS: i64 = 40;
/// Dead air: silence under this level. The programme audio is digitally silent between lines
/// (measured: sbx-20261021-085027-5m finds the same stretches at -35 and -50 dB), so a
/// broadcast silence detector's floor loses nothing. Assumption.
pub const DEAD_AIR_NOISE_DB: f64 = -50.0;
/// Shorter silences are pauses, not dead air. Assumption.
pub const DEAD_AIR_MIN_S: f64 = 1.0;
/// `dead_air` passes when the longest silence is at most this: the say-gap ceiling.
pub const DEAD_AIR_CEILING_S: f64 = 15.0;

fn check(id: SandboxCheckId, state: SandboxCheckState, detail: String) -> SandboxCheck {
    SandboxCheck { id, state, detail }
}

fn unmeasured(id: SandboxCheckId, detail: impl Into<String>) -> SandboxCheck {
    check(id, SandboxCheckState::Unmeasured, detail.into())
}

fn verdict(id: SandboxCheckId, pass: bool, detail: String) -> SandboxCheck {
    let state = if pass {
        SandboxCheckState::Pass
    } else {
        SandboxCheckState::Fail
    };
    check(id, state, detail)
}

fn clock(ts_ms: TsMs) -> String {
    shadowrunr_sim::transcript::clock(ts_ms)
}

// ── the show, from its transcript ───────────────────────────────────────

/// What the watched checks need from `transcript.jsonl`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Show {
    /// The host's `say` lines: their seq (the 1-based transcript line) and time.
    pub says: Vec<(u64, TsMs)>,
    /// The first `sign_off` op.
    pub sign_off_ms: Option<TsMs>,
    /// The session's length on the stream clock.
    pub end_ms: TsMs,
}

impl Show {
    /// From transcript text; a line that does not parse still counts as a seq.
    pub fn parse(transcript: &str, end_ms: TsMs) -> Self {
        let mut show = Show {
            end_ms,
            ..Self::default()
        };
        for (i, line) in transcript.lines().enumerate() {
            match serde_json::from_str::<StreamEvent>(line) {
                Ok(StreamEvent::Say {
                    ts_ms,
                    speaker: Speaker::Host,
                    ..
                }) => show.says.push((i as u64 + 1, ts_ms)),
                Ok(StreamEvent::Op {
                    ts_ms,
                    op: Op::SignOff { .. },
                }) if show.sign_off_ms.is_none() => show.sign_off_ms = Some(ts_ms),
                _ => {}
            }
        }
        show
    }

    /// The show's span on the stream clock: from the cold open (his first line) to the
    /// sign-off, else to the end.
    pub fn span(&self) -> Option<(TsMs, TsMs)> {
        let from = self.says.first()?.1;
        let to = self.sign_off_ms.unwrap_or(self.end_ms).max(from);
        Some((from, to))
    }
}

/// `voice.jsonl` text, line by line; lines that do not parse are skipped.
pub fn parse_voice(text: &str) -> Vec<VoiceLogLine> {
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

// ── voiced ──────────────────────────────────────────────────────────────

/// `voiced`: the share of the host's `say` lines whose every rendered part was `played`, with
/// nothing skipped or dropped. A line the voice never rendered is not voiced; one it dropped
/// because nobody followed the run any more counts as `skipped unfollowed`.
pub fn voiced(show: &Show, voice: Option<&[VoiceLogLine]>) -> SandboxCheck {
    const ID: SandboxCheckId = SandboxCheckId::Voiced;
    let Some(voice) = voice else {
        return unmeasured(ID, "no voice.jsonl: the voice did not follow the session");
    };
    let spoke = voice.iter().any(|l| {
        matches!(
            l.event,
            VoiceLogEvent::Rendered | VoiceLogEvent::Skipped | VoiceLogEvent::Played
        )
    });
    if !spoke {
        return unmeasured(ID, "the voice rendered nothing: it was off");
    }
    if show.says.is_empty() {
        return unmeasured(ID, "the host said nothing");
    }
    let mut rendered: HashMap<u64, HashSet<u32>> = HashMap::new();
    let mut played: HashMap<u64, HashSet<u32>> = HashMap::new();
    let mut skipped: HashMap<u64, String> = HashMap::new();
    let mut dropped: HashSet<u64> = HashSet::new();
    for l in voice {
        match l.event {
            VoiceLogEvent::Rendered => {
                rendered
                    .entry(l.seq)
                    .or_default()
                    .insert(l.part.unwrap_or(0));
            }
            VoiceLogEvent::Played => {
                played.entry(l.seq).or_default().insert(l.part.unwrap_or(0));
            }
            VoiceLogEvent::Skipped => {
                skipped
                    .entry(l.seq)
                    .or_insert_with(|| l.reason.clone().unwrap_or_else(|| "unknown".into()));
            }
            VoiceLogEvent::StageDropped => {
                dropped.insert(l.seq);
            }
            // A queued line is judged by how it ended; `queued` alone says nothing of that.
            VoiceLogEvent::Queued | VoiceLogEvent::Face => {}
        }
    }
    let mut full = 0usize;
    let mut by_reason: BTreeMap<String, usize> = BTreeMap::new();
    let (mut stage_dropped, mut never, mut partial) = (0usize, 0usize, 0usize);
    for (seq, _) in &show.says {
        if let Some(r) = skipped.get(seq) {
            *by_reason.entry(r.clone()).or_default() += 1;
        } else if dropped.contains(seq) {
            stage_dropped += 1;
        } else {
            match rendered.get(seq) {
                None => never += 1,
                Some(parts) => {
                    let got = played.get(seq);
                    if parts.iter().all(|p| got.is_some_and(|g| g.contains(p))) {
                        full += 1;
                    } else {
                        partial += 1;
                    }
                }
            }
        }
    }
    let total = show.says.len();
    let share = full as f64 / total as f64;
    let skipped_txt = if by_reason.is_empty() {
        "skipped 0".to_string()
    } else {
        by_reason
            .iter()
            .map(|(r, n)| format!("skipped {r} {n}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    verdict(
        ID,
        share >= VOICED_MIN_SHARE,
        format!(
            "{full}/{total} lines played in full ({:.0}%; floor {:.0}%): {skipped_txt}, \
             stage_dropped {stage_dropped}, partly played {partial}, never rendered {never}",
            share * 100.0,
            VOICED_MIN_SHARE * 100.0
        ),
    )
}

// ── face ────────────────────────────────────────────────────────────────

/// `face`: the share of the session `[0, end_ms]` the real rig was drawn. Before the first
/// `face` event the state is unknown, which counts as silhouette.
pub fn face(show: &Show, voice: Option<&[VoiceLogLine]>) -> SandboxCheck {
    const ID: SandboxCheckId = SandboxCheckId::Face;
    let events: Vec<(TsMs, bool)> = voice
        .unwrap_or_default()
        .iter()
        .filter(|l| l.event == VoiceLogEvent::Face)
        .map(|l| (l.ts_ms, l.reason.as_deref() == Some("rig")))
        .collect();
    if events.is_empty() {
        return unmeasured(ID, "no face events in voice.jsonl");
    }
    let end = show.end_ms.max(1);
    let (mut at, mut rig, mut rig_ms, mut swaps) = (0, false, 0, 0u32);
    for &(t, is_rig) in &events {
        let t = t.clamp(0, end);
        if rig {
            rig_ms += t - at;
        }
        at = t;
        if is_rig != rig && t > 0 {
            swaps += 1;
        }
        rig = is_rig;
    }
    if rig {
        rig_ms += end - at;
    }
    let share = rig_ms as f64 / end as f64;
    let last = if rig { "rig" } else { "silhouette" };
    verdict(
        ID,
        share >= FACE_MIN_SHARE,
        format!(
            "rig drawn {:.1}% of {} ({:.1} s silhouette or unknown; floor {:.0}%), {swaps} \
             changes, {last} at the end",
            share * 100.0,
            clock(end),
            (end - rig_ms) as f64 / 1000.0,
            FACE_MIN_SHARE * 100.0
        ),
    )
}

// ── the recording ───────────────────────────────────────────────────────

/// One silence `silencedetect` found, in file seconds; `end` is `None` when it runs to the end
/// of the file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Silence {
    pub start: f64,
    pub end: Option<f64>,
}

/// The silences in ffmpeg's `silencedetect` log (its stderr).
pub fn parse_silences(log: &str) -> Vec<Silence> {
    let num = |line: &str, key: &str| -> Option<f64> {
        let rest = &line[line.find(key)? + key.len()..];
        rest.split_whitespace().next()?.parse().ok()
    };
    let mut out: Vec<Silence> = Vec::new();
    for line in log.lines() {
        if let Some(s) = num(line, "silence_start: ") {
            out.push(Silence {
                start: s.max(0.0),
                end: None,
            });
        } else if let Some(e) = num(line, "silence_end: ") {
            match out.last_mut() {
                Some(last) if last.end.is_none() => last.end = Some(e),
                _ => out.push(Silence {
                    start: 0.0,
                    end: Some(e),
                }),
            }
        }
    }
    out
}

/// The file's duration from ffmpeg's log header (`Duration: 00:04:18.23`).
pub fn parse_duration(log: &str) -> Option<f64> {
    let rest = &log[log.find("Duration: ")? + "Duration: ".len()..];
    let hms = rest.split(',').next()?.trim();
    let mut parts = hms.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Run `silencedetect` over the file's audio: its log (stderr).
pub fn silencedetect(
    ffmpeg: &str,
    file: &Path,
    noise_db: f64,
    min_s: f64,
) -> Result<String, String> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-nostats", "-i"])
        .arg(file)
        .args([
            "-vn",
            "-af",
            &format!("silencedetect=noise={noise_db}dB:d={min_s}"),
            "-f",
            "null",
            "-",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::voice::sidecar::CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("{ffmpeg}: {e}"))?;
    let log = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        let tail: String = log.lines().last().unwrap_or("").chars().take(200).collect();
        return Err(format!("ffmpeg failed ({}): {tail}", out.status));
    }
    Ok(log)
}

fn median(v: &mut [i64]) -> i64 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// `av_sync`. Speech onsets (each silence's end, at [`ONSET_NOISE_DB`] over
/// [`ONSET_MIN_SILENCE_S`]) are matched to the `played` times shifted into the file
/// (`t - stream_offset_ms`): each onset to its nearest played time within [`ONSET_MATCH_MS`],
/// one played time per onset. A match's offset is `onset - played` (positive: the sound is
/// later than the stage said it started). Only line starts that follow a pause show as onsets.
///
/// - **The constant** is the median offset over the whole run: the delay of the recording path
///   (OBS's browser source, ~200 ms as REC measured). It is reported, never judged.
/// - **Head** and **tail** are the medians of the first and the last
///   `min(AV_SYNC_WINDOW, matches / 2)` matches.
/// - Pass when head and tail are each within `-AV_SYNC_EARLY_MS..=AV_SYNC_LATE_MS` of the
///   constant and `|tail - head| <= AV_DRIFT_MAX_MS`.
pub fn av_sync(played_ms: &[TsMs], onsets_s: &[f64], stream_offset_ms: i64) -> SandboxCheck {
    const ID: SandboxCheckId = SandboxCheckId::AvSync;
    if played_ms.is_empty() {
        return unmeasured(ID, "no played line in voice.jsonl");
    }
    let file_ms: Vec<i64> = played_ms.iter().map(|t| t - stream_offset_ms).collect();
    // onset index -> (|offset|, played index, offset)
    let mut best: BTreeMap<usize, (i64, usize, i64)> = BTreeMap::new();
    for (pi, &p) in file_ms.iter().enumerate() {
        let near = onsets_s
            .iter()
            .enumerate()
            .map(|(oi, &o)| (oi, (o * 1000.0).round() as i64 - p))
            .min_by_key(|(_, d)| d.abs());
        if let Some((oi, d)) = near.filter(|(_, d)| d.abs() <= ONSET_MATCH_MS) {
            let e = best.entry(oi).or_insert((d.abs(), pi, d));
            if d.abs() < e.0 {
                *e = (d.abs(), pi, d);
            }
        }
    }
    let mut pairs: Vec<(usize, i64)> = best.values().map(|&(_, pi, d)| (pi, d)).collect();
    pairs.sort_unstable();
    if pairs.len() < AV_SYNC_MIN_MATCHES {
        return unmeasured(
            ID,
            format!(
                "{} of {} played parts matched a speech onset (need {AV_SYNC_MIN_MATCHES}; \
                 {} onsets found)",
                pairs.len(),
                played_ms.len(),
                onsets_s.len()
            ),
        );
    }
    let offs: Vec<i64> = pairs.iter().map(|p| p.1).collect();
    let n = AV_SYNC_WINDOW.min(offs.len() / 2);
    let constant = median(&mut offs.clone());
    let head = median(&mut offs[..n].to_vec());
    let tail = median(&mut offs[offs.len() - n..].to_vec());
    let (dh, dt, drift) = (head - constant, tail - constant, tail - head);
    let within = |d: i64| (-AV_SYNC_EARLY_MS..=AV_SYNC_LATE_MS).contains(&d);
    let mut spread = offs.clone();
    spread.sort_unstable();
    verdict(
        ID,
        within(dh) && within(dt) && drift.abs() <= AV_DRIFT_MAX_MS,
        format!(
            "head {head:+} ms, tail {tail:+} ms (median of {n} line starts each): {dh:+} and \
             {dt:+} ms from the run's constant (window -{AV_SYNC_EARLY_MS}/+{AV_SYNC_LATE_MS}), \
             drift {drift:+} ms (max {AV_DRIFT_MAX_MS}); constant {constant:+} ms, the \
             recording path's delay (median of {} matched of {} played parts, range {:+}..{:+} \
             ms; reported, not judged)",
            offs.len(),
            played_ms.len(),
            spread[0],
            spread[spread.len() - 1]
        ),
    )
}

/// `dead_air`: the longest silence (at [`DEAD_AIR_NOISE_DB`], at least [`DEAD_AIR_MIN_S`])
/// inside the show's span ([`Show::span`]: the cold open's first line to the sign-off, on the
/// stream clock, shifted into the file by `stream_offset_ms`). Each silence is clipped to the
/// span; one running to the end of the file ends at `file_s`.
pub fn dead_air(
    show: &Show,
    silences: &[Silence],
    stream_offset_ms: i64,
    file_s: Option<f64>,
) -> SandboxCheck {
    const ID: SandboxCheckId = SandboxCheckId::DeadAir;
    let Some((from, to)) = show.span() else {
        return unmeasured(ID, "the host said nothing: no show to measure");
    };
    let (a, b) = (
        (from - stream_offset_ms) as f64 / 1000.0,
        (to - stream_offset_ms) as f64 / 1000.0,
    );
    let mut longest: Option<(f64, f64)> = None;
    let mut count = 0;
    for s in silences {
        let end = s.end.or(file_s).unwrap_or(b);
        let (lo, hi) = (s.start.max(a), end.min(b));
        if hi - lo < DEAD_AIR_MIN_S {
            continue;
        }
        count += 1;
        if longest.is_none_or(|(l, h)| hi - lo > h - l) {
            longest = Some((lo, hi));
        }
    }
    let short = file_s
        .filter(|f| *f < b)
        .map(|f| format!("; the recording ends {:.1} s before the sign-off", b - f))
        .unwrap_or_default();
    let floor = format!("at {DEAD_AIR_NOISE_DB} dB, at least {DEAD_AIR_MIN_S} s");
    let at = |s: f64| clock((s * 1000.0) as i64 + stream_offset_ms);
    match longest {
        None => verdict(
            ID,
            true,
            format!(
                "no silence {floor} from {} to {} (ceiling {DEAD_AIR_CEILING_S} s){short}",
                clock(from),
                clock(to)
            ),
        ),
        Some((lo, hi)) => verdict(
            ID,
            hi - lo <= DEAD_AIR_CEILING_S,
            format!(
                "longest silence {:.1} s, from {} to {} ({count} silences {floor}, between the \
                 cold open at {} and the sign-off at {}; ceiling {DEAD_AIR_CEILING_S} s){short}",
                hi - lo,
                at(lo),
                at(hi),
                clock(from),
                clock(to)
            ),
        ),
    }
}

// ── one run dir ─────────────────────────────────────────────────────────

/// The run's recording: `recording.json`, and the file in the run dir (by its name there, so
/// a moved run dir still finds it).
pub fn recording_in(dir: &Path) -> Result<(Recording, PathBuf), String> {
    let meta = dir.join(META_FILE);
    let text = std::fs::read_to_string(&meta).map_err(|_| "no recording.json".to_string())?;
    let rec: Recording =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", meta.display()))?;
    let name = Path::new(&rec.file.replace('\\', "/"))
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&rec.file));
    let file = dir.join(name);
    if !file.is_file() {
        return Err(format!(
            "recording.json names {}, not in the run dir",
            rec.file
        ));
    }
    Ok((rec, file))
}

/// Measure the four watched checks for the session in `dir` (its `report` gives the clock).
/// `ffmpeg` is the command that reads the recording.
pub fn measure(dir: &Path, report: &SandboxReport, ffmpeg: &str) -> Vec<SandboxCheck> {
    let end_ms = (report.ended_ms - report.started_ms).max(0);
    let transcript = std::fs::read_to_string(dir.join("transcript.jsonl")).unwrap_or_default();
    let show = Show::parse(&transcript, end_ms);
    let voice = std::fs::read_to_string(dir.join(crate::voice::log::FILE))
        .ok()
        .map(|t| parse_voice(&t));
    let mut checks = vec![
        voiced(&show, voice.as_deref()),
        face(&show, voice.as_deref()),
    ];
    let recording = recording_in(dir);
    let (av, air) = match recording {
        Err(why) => {
            let why = if report.spec.record {
                why
            } else {
                "the session was not recorded".into()
            };
            (
                unmeasured(SandboxCheckId::AvSync, why.clone()),
                unmeasured(SandboxCheckId::DeadAir, why),
            )
        }
        Ok((rec, file)) => {
            let played: Vec<TsMs> = voice
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter(|l| l.event == VoiceLogEvent::Played)
                .map(|l| l.ts_ms)
                .collect();
            let av = match silencedetect(ffmpeg, &file, ONSET_NOISE_DB, ONSET_MIN_SILENCE_S) {
                Ok(log) => {
                    let onsets: Vec<f64> =
                        parse_silences(&log).iter().filter_map(|s| s.end).collect();
                    av_sync(&played, &onsets, rec.stream_offset_ms)
                }
                Err(e) => unmeasured(SandboxCheckId::AvSync, e),
            };
            let air = match silencedetect(ffmpeg, &file, DEAD_AIR_NOISE_DB, DEAD_AIR_MIN_S) {
                Ok(log) => dead_air(
                    &show,
                    &parse_silences(&log),
                    rec.stream_offset_ms,
                    parse_duration(&log),
                ),
                Err(e) => unmeasured(SandboxCheckId::DeadAir, e),
            };
            (av, air)
        }
    };
    checks.push(av);
    checks.push(air);
    checks
}

/// Put `checks` into `report` (replacing those ids) and recompute its gate.
pub fn merge(report: &mut SandboxReport, checks: Vec<SandboxCheck>) {
    for c in checks {
        match report.checks.iter_mut().find(|x| x.id == c.id) {
            Some(slot) => *slot = c,
            None => report.checks.push(c),
        }
    }
    report.pass = gate_pass(&report.spec, &report.checks);
}

/// Recompute the watched checks of the session in `dir` and rewrite its `session.json`
/// (`write`), returning the report.
pub fn refresh(dir: &Path, ffmpeg: &str, write: bool) -> Result<SandboxReport, String> {
    let path = dir.join(shadowrunr_sim::sandbox::SESSION_FILE);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut report: SandboxReport =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let checks = measure(dir, &report, ffmpeg);
    merge(&mut report, checks);
    if write {
        crate::sandbox::write_durable(&path, &report)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::sandbox::SandboxStartParams;

    fn line(json: &str) -> VoiceLogLine {
        serde_json::from_str(json).unwrap()
    }

    fn show(says: &[(u64, TsMs)], sign_off: Option<TsMs>, end_ms: TsMs) -> Show {
        Show {
            says: says.to_vec(),
            sign_off_ms: sign_off,
            end_ms,
        }
    }

    fn state(c: &SandboxCheck) -> SandboxCheckState {
        c.state
    }

    #[test]
    fn the_show_comes_from_the_transcript_by_line() {
        let t = [
            r#"{"type":"stream_start","ts_ms":0,"stream_id":"s","character":"malakor","tone_variant":"theatrical","core_tier":"haiku","audience_size":30}"#,
            r#"{"type":"say","ts_ms":900,"speaker":"malakor","text":"Rise."}"#,
            r#"{"type":"op","ts_ms":950,"op":{"op":"sign_off","reason":"done"}}"#,
            "not json",
            r#"{"type":"say","ts_ms":1200,"speaker":"malakor","text":"Kneel."}"#,
        ]
        .join("\n");
        let s = Show::parse(&t, 5_000);
        assert_eq!(s.says, [(2, 900), (5, 1200)]);
        assert_eq!(s.sign_off_ms, Some(950));
        assert_eq!(s.span(), Some((900, 950)));
        assert_eq!(Show::parse("", 5_000).span(), None);
    }

    #[test]
    fn voiced_counts_lines_played_in_full_and_why_the_rest_were_not() {
        let s = show(&[(4, 1), (5, 2), (6, 3), (7, 4), (8, 5)], None, 10_000);
        let v: Vec<VoiceLogLine> = [
            r#"{"ts_ms":1,"seq":4,"event":"rendered","part":0,"duration_ms":10}"#,
            r#"{"ts_ms":1,"seq":4,"event":"rendered","part":1,"duration_ms":10}"#,
            r#"{"ts_ms":2,"seq":4,"event":"played","part":0}"#,
            r#"{"ts_ms":2,"seq":4,"event":"played","part":1}"#,
            r#"{"ts_ms":3,"seq":5,"event":"rendered","part":0,"duration_ms":10}"#,
            r#"{"ts_ms":3,"seq":5,"event":"rendered","part":1,"duration_ms":10}"#,
            r#"{"ts_ms":4,"seq":5,"event":"played","part":0}"#,
            r#"{"ts_ms":5,"seq":6,"event":"skipped","part":0,"reason":"late"}"#,
            r#"{"ts_ms":5,"seq":7,"event":"rendered","part":0,"duration_ms":10}"#,
            r#"{"ts_ms":6,"seq":7,"event":"stage_dropped","reason":"stage_cap"}"#,
        ]
        .iter()
        .map(|j| line(j))
        .collect();
        let c = voiced(&s, Some(&v));
        assert_eq!(state(&c), SandboxCheckState::Fail);
        assert_eq!(
            c.detail,
            "1/5 lines played in full (20%; floor 90%): skipped late 1, stage_dropped 1, \
             partly played 1, never rendered 1"
        );
        // Nine of ten in full is the floor.
        let says: Vec<(u64, TsMs)> = (1..=10).map(|i| (i, i as TsMs)).collect();
        let mut v: Vec<VoiceLogLine> = Vec::new();
        for i in 1..=10u64 {
            v.push(line(&format!(
                r#"{{"ts_ms":1,"seq":{i},"event":"rendered","part":0,"duration_ms":10}}"#
            )));
            if i != 3 {
                v.push(line(&format!(
                    r#"{{"ts_ms":2,"seq":{i},"event":"played","part":0}}"#
                )));
            }
        }
        let c = voiced(&show(&says, None, 10_000), Some(&v));
        assert_eq!(state(&c), SandboxCheckState::Pass, "{}", c.detail);
        // Voice off: unmeasured.
        assert_eq!(state(&voiced(&s, None)), SandboxCheckState::Unmeasured);
        let face_only = [line(r#"{"ts_ms":0,"seq":0,"event":"face","reason":"rig"}"#)];
        assert_eq!(
            state(&voiced(&s, Some(&face_only))),
            SandboxCheckState::Unmeasured
        );
    }

    #[test]
    fn queued_lines_change_neither_voiced_nor_the_clip_timing() {
        let s = show(&[(4, 1), (5, 2), (6, 3), (7, 4), (8, 5)], None, 10_000);
        let before = [
            r#"{"ts_ms":1,"seq":4,"event":"rendered","part":0,"duration_ms":10}"#,
            r#"{"ts_ms":1,"seq":4,"event":"rendered","part":1,"duration_ms":10}"#,
            r#"{"ts_ms":2,"seq":4,"event":"played","part":0}"#,
            r#"{"ts_ms":2,"seq":4,"event":"played","part":1}"#,
            r#"{"ts_ms":3,"seq":5,"event":"rendered","part":0,"duration_ms":10}"#,
            r#"{"ts_ms":4,"seq":5,"event":"played","part":0}"#,
            r#"{"ts_ms":5,"seq":6,"event":"skipped","part":0,"reason":"late"}"#,
            r#"{"ts_ms":5,"seq":7,"event":"rendered","part":0,"duration_ms":10}"#,
            r#"{"ts_ms":6,"seq":7,"event":"stage_dropped","reason":"stage_cap"}"#,
        ]
        .join("\n");
        let mut with = String::new();
        for seq in [4, 5, 6, 7, 8] {
            with.push_str(&format!(
                "{{\"ts_ms\":0,\"seq\":{seq},\"event\":\"queued\",\"text\":\"x\"}}\n"
            ));
        }
        with.push_str(&before);
        let a = voiced(&s, Some(&parse_voice(&before)));
        let b = voiced(&s, Some(&parse_voice(&with)));
        assert_eq!((state(&a), &a.detail), (state(&b), &b.detail));
        assert_eq!(
            shadowrunr_clips::voice::parse(&before),
            shadowrunr_clips::voice::parse(&with)
        );
    }

    #[test]
    fn face_counts_unknown_time_as_silhouette() {
        let s = show(&[], None, 100_000);
        // The rig from 2 s: 98% drawn.
        let v = [
            line(r#"{"ts_ms":-500,"seq":0,"event":"face","reason":"silhouette"}"#),
            line(r#"{"ts_ms":2000,"seq":0,"event":"face","reason":"rig"}"#),
        ];
        let c = face(&s, Some(&v));
        assert_eq!(state(&c), SandboxCheckState::Pass, "{}", c.detail);
        assert!(
            c.detail.starts_with("rig drawn 98.0% of 01:40 (2.0 s"),
            "{}",
            c.detail
        );
        // First report at 10 s: the 10 s before are unknown, so silhouette, and it fails.
        let v = [line(
            r#"{"ts_ms":10000,"seq":0,"event":"face","reason":"rig"}"#,
        )];
        assert_eq!(state(&face(&s, Some(&v))), SandboxCheckState::Fail);
        // A drop to the silhouette for 6 s mid-show fails too.
        let v = [
            line(r#"{"ts_ms":0,"seq":0,"event":"face","reason":"rig"}"#),
            line(r#"{"ts_ms":50000,"seq":0,"event":"face","reason":"silhouette"}"#),
            line(r#"{"ts_ms":56000,"seq":0,"event":"face","reason":"rig"}"#),
        ];
        let c = face(&s, Some(&v));
        assert_eq!(state(&c), SandboxCheckState::Fail, "{}", c.detail);
        assert!(
            c.detail.contains("2 changes, rig at the end"),
            "{}",
            c.detail
        );
        assert_eq!(state(&face(&s, Some(&[]))), SandboxCheckState::Unmeasured);
    }

    #[test]
    fn silencedetect_logs_parse() {
        let log = "  Duration: 00:04:18.23, start: 0.000000, bitrate: 6118 kb/s\n\
            [silencedetect @ 0x1] silence_start: 0\n\
            [silencedetect @ 0x1] silence_end: 11.477375 | silence_duration: 11.477375\n\
            [silencedetect @ 0x1] silence_start: 20.562687\n\
            [silencedetect @ 0x1] silence_end: 23.977 | silence_duration: 3.414312\n\
            [silencedetect @ 0x1] silence_start: 253.938375\n";
        assert_eq!(
            parse_silences(log),
            [
                Silence {
                    start: 0.0,
                    end: Some(11.477375)
                },
                Silence {
                    start: 20.562687,
                    end: Some(23.977)
                },
                Silence {
                    start: 253.938375,
                    end: None
                },
            ]
        );
        assert_eq!(parse_duration(log), Some(258.23));
    }

    /// Played times every 10 s from 10 s; onsets `delay(i)` ms after each, in a file whose
    /// first frame is at stream time -500.
    fn onsets(n: usize, delay: impl Fn(usize) -> i64) -> (Vec<TsMs>, Vec<f64>) {
        let played: Vec<TsMs> = (0..n).map(|i| 10_000 + 10_000 * i as i64).collect();
        let on = played
            .iter()
            .enumerate()
            .map(|(i, p)| (p + 500 + delay(i)) as f64 / 1000.0)
            .collect();
        (played, on)
    }

    #[test]
    fn av_sync_judges_drift_and_spread_not_the_constant() {
        // A steady 200 ms path: passes, and the constant is reported.
        let (p, o) = onsets(20, |i| 200 + if i % 2 == 0 { 10 } else { -10 });
        let c = av_sync(&p, &o, -500);
        assert_eq!(state(&c), SandboxCheckState::Pass, "{}", c.detail);
        assert!(c.detail.contains("constant +210 ms"), "{}", c.detail);
        assert!(c.detail.contains("drift +0 ms"), "{}", c.detail);
        // Drifting 5 ms a line: the tail is 80+ ms behind the head.
        let (p, o) = onsets(20, |i| 200 + 5 * i as i64);
        let c = av_sync(&p, &o, -500);
        assert_eq!(state(&c), SandboxCheckState::Fail, "{}", c.detail);
        // A late tail beyond the window around the constant.
        let (p, o) = onsets(20, |i| if i >= 14 { 400 } else { 200 });
        let c = av_sync(&p, &o, -500);
        assert_eq!(state(&c), SandboxCheckState::Fail, "{}", c.detail);
        // An onset further than the match window is no match; too few is unmeasured.
        let (p, o) = onsets(6, |i| if i < 3 { 900 } else { 200 });
        assert_eq!(state(&av_sync(&p, &o, -500)), SandboxCheckState::Unmeasured);
        assert_eq!(state(&av_sync(&[], &o, 0)), SandboxCheckState::Unmeasured);
    }

    #[test]
    fn av_sync_matches_one_played_part_per_onset() {
        // Two parts 300 ms apart and one onset after the first: it matches the nearer one.
        let played = [10_000, 10_300, 20_000, 30_000, 40_000, 50_000];
        let o = [10.2, 20.2, 30.2, 40.2, 50.2];
        let c = av_sync(&played, &o, 0);
        assert_eq!(state(&c), SandboxCheckState::Pass, "{}", c.detail);
        assert!(
            c.detail.contains("median of 5 matched of 6"),
            "{}",
            c.detail
        );
    }

    #[test]
    fn dead_air_is_the_longest_silence_inside_the_show() {
        // The show runs 10 s to 200 s on the stream clock; the file starts at stream -500.
        let s = show(&[(3, 10_000), (9, 150_000)], Some(200_000), 215_000);
        let sil = |a: f64, b: Option<f64>| Silence { start: a, end: b };
        let silences = [
            sil(0.0, Some(12.0)),    // before the cold open: clipped to 10.5-12.0
            sil(60.5, Some(68.0)),   // 7.5 s
            sil(120.5, Some(120.9)), // a pause
            sil(195.0, None),        // to the end of the file, clipped at the sign-off
        ];
        let c = dead_air(&s, &silences, -500, Some(230.0));
        assert_eq!(state(&c), SandboxCheckState::Pass, "{}", c.detail);
        assert!(
            c.detail
                .starts_with("longest silence 7.5 s, from 01:00 to 01:07 (3 silences"),
            "{}",
            c.detail
        );
        let long = [sil(40.5, Some(56.5))];
        let c = dead_air(&s, &long, -500, Some(230.0));
        assert_eq!(state(&c), SandboxCheckState::Fail, "{}", c.detail);
        assert!(
            c.detail.starts_with("longest silence 16.0 s, from 00:40"),
            "{}",
            c.detail
        );
        // Silent from the cold open to a sign-off that never came: the end of the session.
        let s = show(&[(3, 10_000)], None, 60_000);
        let c = dead_air(&s, &[sil(0.0, None)], 0, Some(70.0));
        assert_eq!(state(&c), SandboxCheckState::Fail);
        assert!(
            c.detail.starts_with("longest silence 50.0 s"),
            "{}",
            c.detail
        );
        assert_eq!(
            state(&dead_air(&show(&[], None, 1), &[], 0, None)),
            SandboxCheckState::Unmeasured
        );
    }

    fn report(record: bool) -> SandboxReport {
        let checks = SandboxCheckId::ALL
            .iter()
            .map(|&id| SandboxCheck {
                id,
                state: SandboxCheckState::Pass,
                detail: String::new(),
            })
            .collect();
        SandboxReport {
            session_id: "sbx-1".into(),
            spec: SandboxStartParams {
                minutes: 5,
                seed: None,
                chaos: None,
                judge: true,
                tone: None,
                record,
            },
            started_ms: 1_000,
            ended_ms: 61_000,
            pass: true,
            checks,
            max_say_gap_s: 1.0,
            say_gap_p95_s: 1.0,
            beat_ms_p50: 1,
            beat_ms_p95: 1,
            core_errors: 0,
            stall_lines: 0,
            health_transitions: 0,
            judge: None,
        }
    }

    #[test]
    fn an_unrecorded_voice_off_run_gates_as_before_and_a_recorded_one_does_not() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("transcript.jsonl"),
            r#"{"type":"say","ts_ms":900,"speaker":"malakor","text":"Rise."}"#,
        )
        .unwrap();
        // Unrecorded, voice off: every watched check is unmeasured and the gate holds.
        let mut r = report(false);
        let checks = measure(d.path(), &r, "ffmpeg-not-called");
        assert!(checks
            .iter()
            .all(|c| c.state == SandboxCheckState::Unmeasured));
        assert_eq!(checks[2].detail, "the session was not recorded");
        merge(&mut r, checks);
        assert!(r.pass, "{:?}", r.checks);
        // Recorded, but the recording is missing: the same checks fail the gate.
        let mut r = report(true);
        let checks = measure(d.path(), &r, "ffmpeg-not-called");
        assert_eq!(checks[2].detail, "no recording.json");
        merge(&mut r, checks);
        assert!(!r.pass);
    }

    #[test]
    fn a_recorded_run_with_every_watched_check_green_passes_its_gate() {
        // The all-green path end to end through the run dir: voice.jsonl, recording.json and a
        // stand-in for ffmpeg that prints a silencedetect log.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        let mut transcript = Vec::new();
        let mut voice = vec![r#"{"ts_ms":-400,"seq":0,"event":"face","reason":"rig"}"#.to_string()];
        let mut silences = String::from("  Duration: 00:01:05.00, start: 0.000000\n");
        let mut last_end = 0.0;
        for i in 0..10u64 {
            let t = 5_000 + 5_000 * i as i64;
            transcript.push(format!(
                r#"{{"type":"say","ts_ms":{t},"speaker":"malakor","text":"Line {i}."}}"#
            ));
            let seq = i + 1;
            voice.push(format!(
                r#"{{"ts_ms":{t},"seq":{seq},"event":"rendered","part":0,"duration_ms":3000}}"#
            ));
            voice.push(format!(
                r#"{{"ts_ms":{},"seq":{seq},"event":"played","part":0}}"#,
                t + 50
            ));
            // The file starts at stream -500; speech starts 200 ms after `played`.
            let onset = (t + 50 + 500 + 200) as f64 / 1000.0;
            silences.push_str(&format!(
                "[silencedetect @ 0x1] silence_start: {last_end}\n[silencedetect @ 0x1] silence_end: {onset} | silence_duration: 1\n"
            ));
            last_end = onset + 3.0;
        }
        transcript
            .push(r#"{"type":"op","ts_ms":55000,"op":{"op":"sign_off","reason":"bye"}}"#.into());
        std::fs::write(dir.join("transcript.jsonl"), transcript.join("\n")).unwrap();
        std::fs::write(dir.join("voice.jsonl"), voice.join("\n")).unwrap();
        std::fs::write(dir.join("recording.mp4"), b"media").unwrap();
        let rec = Recording::new(&dir.join("recording.mp4"), 500, 66_000, 1_000, "u");
        rec.write(dir).unwrap();
        let log = dir.join("log.txt");
        std::fs::write(&log, &silences).unwrap();
        let ffmpeg = fake_ffmpeg(dir, &log);
        let mut r = report(true);
        r.checks.retain(|c| !c.id.is_watched());
        std::fs::write(dir.join("session.json"), serde_json::to_string(&r).unwrap()).unwrap();
        let r = refresh(dir, &ffmpeg, true).unwrap();
        for id in ["voiced", "face", "av_sync", "dead_air"] {
            let c = r.checks.iter().find(|c| c.id.as_str() == id).unwrap();
            assert_eq!(c.state, SandboxCheckState::Pass, "{id}: {}", c.detail);
        }
        assert!(r.pass);
        let on_disk: SandboxReport =
            serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap())
                .unwrap();
        assert_eq!(on_disk, r, "session.json is the report");
    }

    /// A script standing in for ffmpeg: prints `log` to stderr.
    fn fake_ffmpeg(dir: &Path, log: &Path) -> String {
        if cfg!(windows) {
            let p = dir.join("ffmpeg.cmd");
            std::fs::write(&p, format!("@type \"{}\" 1>&2\r\n", log.display())).unwrap();
            p.to_string_lossy().into_owned()
        } else {
            let p = dir.join("ffmpeg.sh");
            std::fs::write(&p, format!("#!/bin/sh\ncat '{}' >&2\n", log.display())).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            p.to_string_lossy().into_owned()
        }
    }
}
