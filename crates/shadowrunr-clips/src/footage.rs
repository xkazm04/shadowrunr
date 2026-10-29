//! Cuts from the show's own recording (docs/guide/recording.md, "Clips from the recording").
//!
//! A recorded run has `recording.json` beside its `recording.<ext>`: OBS rendered the stage
//! itself, so the file holds the broadcast picture (with the stage's own captions and its AI
//! badge) and the host's voice on one clock. A stream-clock time `t` sits at
//! `t - stream_offset_ms` in the file. From it a render makes:
//!
//! - `cut-16x9.mp4`: the window trimmed out of the recording, re-encoded H.264/AAC so the cut
//!   is frame-accurate, with nothing burnt in (the stage's captions are in the picture). A
//!   SEG-02A cut is the one exception: its ALLEGED header, grade badge and citation strip are
//!   burnt in, because the gate requires them on every published picture.
//! - `cut-9x16.mp4`, 1080x1920: the stage's face region on top (the region rects of
//!   `apps/stage/src/layouts.ts` for the segment's layout), the stage's own AI badge copied
//!   from the picture above it, and the lines burnt in below as ASS, timed by the voice.
//!
//! The programme level of a window is the audio signal ([`rms_db`]).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use shadowrunr_types::clips::{ClipCaption, ClipRecording, ClipWindow};
use shadowrunr_types::SegmentId;

use crate::host::{is_host, ClipHost};
use crate::render::{quiet, ClipHeader};

/// Speech starts this long after its `played` time.
pub const AV_OFFSET_MS: i64 = 200;
/// Where [`AV_OFFSET_MS`] comes from, written into every manifest beside it.
pub const AV_OFFSET_PROVENANCE: &str = "measured on the REC session sbx-20261021-085027-5m (2026-10-21): speech onsets (silencedetect) sit +200 ms (within 25 ms) after their `played` time, the same at the head and the tail, so no drift; a tolerance the pre-roll absorbs, so cut points are not shifted by it";
/// The level spread that maps onto the whole 0..1 audio signal: a window 6 dB over the
/// stream's own level reads 1, 6 dB under reads 0.
pub const AUDIO_SPREAD_DB: f64 = 6.0;
/// What the audio signal assumes. Ungated RMS on purpose: EBU R128 integrated loudness gates
/// out everything 10 LU below the programme, so a voice-only show reads the same in every
/// window; the ungated level also counts pauses, so a dense, loud stretch reads high.
pub const AUDIO_ASSUMPTION: &str = "assumption: a window whose programme audio is louder and denser than the show around it (ungated RMS level, ffmpeg astats, against the whole recording's) is livelier; 6 dB either side spans the signal; not yet scored against an editor's picks";
/// Silence reads this level (astats reports -inf).
const FLOOR_DB: f64 = -90.0;

/// The vertical reframe's frame.
pub const V_WIDTH: u32 = 1080;
pub const V_HEIGHT: u32 = 1920;

/// The recording's file names a run directory may hold, preferred first.
const RECORDING_FILES: [&str; 2] = ["recording.mp4", "recording.mkv"];

#[derive(Deserialize)]
struct RecordingJson {
    #[serde(default)]
    file: String,
    stream_offset_ms: i64,
    width: u32,
    height: u32,
    fps: u32,
}

/// The run's recording, when `recording.json` names a file that exists: the run directory's
/// own `recording.<ext>` first (a moved run dir still finds it), then the recorded path.
pub fn read_recording(dir: &Path) -> Option<ClipRecording> {
    let text = std::fs::read_to_string(dir.join("recording.json")).ok()?;
    let r: RecordingJson = serde_json::from_str(&text).ok()?;
    let file = RECORDING_FILES
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
        .or_else(|| Some(PathBuf::from(&r.file)).filter(|p| p.is_file()))?;
    Some(ClipRecording {
        file: file.to_string_lossy().into_owned(),
        stream_offset_ms: r.stream_offset_ms,
        width: r.width,
        height: r.height,
        fps: r.fps,
        av_offset_ms: AV_OFFSET_MS,
    })
}

/// Where a window sits in the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// File time of the first frame.
    pub in_ms: i64,
    pub dur_ms: i64,
    /// How far the captions' clock (0 = the window's start) moves on the cut's: negative when
    /// the window opens before the recording does and its head is lost.
    pub shift_ms: i64,
}

/// The window on the file's clock. The window's edges already sit on heard speech (see
/// `window::edge_start`), so the trim takes them as they are and is never moved by the
/// voice's [`AV_OFFSET_MS`].
pub fn span(rec: &ClipRecording, w: &ClipWindow) -> Span {
    let len = w.spoken_end_ms - w.spoken_start_ms;
    let in_ms = w.spoken_start_ms - rec.stream_offset_ms;
    let lost = (-in_ms).max(0);
    Span {
        in_ms: in_ms.max(0),
        dur_ms: (len - lost).max(1),
        shift_ms: -lost,
    }
}

fn secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

// ----- level ------------------------------------------------------------------------------

/// The overall RMS level (dBFS) in ffmpeg `astats`' report: the last `RMS level dB:` line.
pub fn parse_rms(stderr: &str) -> Option<f64> {
    stderr
        .lines()
        .filter_map(|l| l.split_once("RMS level dB:").map(|(_, v)| v.trim()))
        .filter_map(|v| v.parse::<f64>().ok())
        .map(|v| {
            if v.is_finite() {
                v.max(FLOOR_DB)
            } else {
                FLOOR_DB
            }
        })
        .next_back()
}

/// The RMS level of `file`'s first audio stream, over `[from, from + dur)` ms or all of it.
/// Silence reads [`FLOOR_DB`].
pub fn rms_db(ffmpeg: &str, file: &Path, window: Option<(i64, i64)>) -> Result<f64, String> {
    let mut cmd = Command::new(ffmpeg);
    quiet(&mut cmd).args(["-hide_banner", "-nostats"]);
    if let Some((from, _)) = window {
        cmd.args(["-ss", &secs(from.max(0))]);
    }
    cmd.arg("-i").arg(file);
    if let Some((_, dur)) = window {
        cmd.args(["-t", &secs(dur.max(1))]);
    }
    let out = cmd
        .args([
            "-vn",
            "-map",
            "0:a:0",
            "-af",
            "astats=measure_perchannel=none",
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let err = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let tail: Vec<&str> = err.lines().rev().take(3).collect();
        return Err(format!("ffmpeg astats failed: {}", tail.join(" | ")));
    }
    parse_rms(&err).ok_or_else(|| "ffmpeg astats printed no RMS level (no audio?)".into())
}

/// The audio signal: the window's level against the stream's, over [`AUDIO_SPREAD_DB`].
pub fn audio_value(window_db: f64, stream_db: f64) -> f64 {
    let v = (window_db - stream_db + AUDIO_SPREAD_DB) / (2.0 * AUDIO_SPREAD_DB);
    (v.clamp(0.0, 1.0) * 1000.0).round() / 1000.0
}

// ----- the vertical reframe ---------------------------------------------------------------

/// A rect in the stage's 1920x1080 canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Where the vertical frame takes its picture from, for one stage layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reframe {
    /// The stage layout (`apps/stage/src/layouts.ts`).
    pub layout: &'static str,
    /// The region scaled to the frame's full width at the top.
    pub main: Rect,
    /// A second region drawn small over the main one: `(rect, size, x, y)` in frame pixels.
    pub inset: Option<(Rect, u32, u32, u32)>,
    /// Where the captions start (frame y).
    pub caption_top: u32,
}

/// The host screen's `avatar` region (layouts.ts `HOST_AVATAR`: `{ x: 64, y: TOP + 8, w: FACE_PX,
/// h: 612 }`, `TOP` 104, `FACE_PX` 560): the face, its glow and the "synthetic voice" label.
pub const IMPROV_FACE: Rect = Rect {
    x: 64,
    y: 112,
    w: 560,
    h: 612,
};
/// The content screens' main area (layouts.ts `CONTENT`: `{ x: GUTTER, y: TOP, w: RIGHT_X -
/// GUTTER - 24, h: 804 }`): the hexmap, the browser window or the desk, whichever is up.
pub const CONTENT_SCREEN: Rect = Rect {
    x: 32,
    y: 104,
    w: 1256,
    h: 804,
};
/// The content screens' corner `avatar` region (layouts.ts `CORNER_AVATAR`).
pub const CORNER_FACE: Rect = Rect {
    x: 16,
    y: 632,
    w: 448,
    h: 448,
};
/// The stage's disclosure badge (stage-kit `Overlays`, `.disclosure`: right 32, top 38), right
/// aligned, with room for its longest text.
pub const BADGE: Rect = Rect {
    x: 1584,
    y: 30,
    w: 320,
    h: 52,
};
/// The main region's top in the vertical frame; the badge sits above it.
const MAIN_TOP: u32 = 170;
const BADGE_TOP: u32 = 60;
/// The badge is drawn at 1.5x.
const BADGE_SCALE_NUM: u32 = 3;
const BADGE_SCALE_DEN: u32 = 2;

/// The reframe for a segment's stage screens (SEG-02A shows the content screens with the host in
/// the corner; everything else is the host screen's big face).
pub fn reframe_for(segment: SegmentId) -> Reframe {
    let scaled_h = |r: Rect| r.h * V_WIDTH / r.w;
    match segment {
        SegmentId::Seg02aHexmap => {
            let bottom = MAIN_TOP + scaled_h(CONTENT_SCREEN);
            Reframe {
                layout: "hexmap",
                main: CONTENT_SCREEN,
                // He stands under the screen, at the left, and the captions follow him.
                inset: Some((CORNER_FACE, 420, 24, bottom + 16)),
                caption_top: bottom + 16 + 420 + 40,
            }
        }
        _ => Reframe {
            layout: "improv",
            main: IMPROV_FACE,
            inset: None,
            caption_top: MAIN_TOP + scaled_h(IMPROV_FACE) + 40,
        },
    }
}

/// The ffmpeg filter graph: a dark ground, the main region, the inset, the badge, then the
/// captions file (`ass`, a name in the working directory).
pub fn vertical_filter(rf: &Reframe, fps: u32, dur_ms: i64, ass: &str) -> String {
    let crop = |r: Rect| format!("crop={}:{}:{}:{}", r.w, r.h, r.x, r.y);
    let bw = BADGE.w * BADGE_SCALE_NUM / BADGE_SCALE_DEN;
    let mut g = format!(
        "color=c=0x060807:s={V_WIDTH}x{V_HEIGHT}:r={fps}:d={}[bg];",
        secs(dur_ms)
    );
    let n = if rf.inset.is_some() { 3 } else { 2 };
    g.push_str(&format!(
        "[0:v]split={n}[m][b]{};",
        if n == 3 { "[i]" } else { "" }
    ));
    g.push_str(&format!(
        "[m]{},scale={V_WIDTH}:-2:flags=lanczos[main];",
        crop(rf.main)
    ));
    g.push_str(&format!(
        "[b]{},scale={bw}:-2:flags=lanczos[badge];",
        crop(BADGE)
    ));
    g.push_str(&format!("[bg][main]overlay=0:{MAIN_TOP}:shortest=1[t1];"));
    let mut last = "t1";
    if let Some((r, size, x, y)) = rf.inset {
        g.push_str(&format!(
            "[i]{},scale={size}:{size}:flags=lanczos[inset];[t1][inset]overlay={x}:{y}[t2];",
            crop(r)
        ));
        last = "t2";
    }
    g.push_str(&format!(
        "[{last}][badge]overlay=(W-w)/2:{BADGE_TOP}[t3];[t3]subtitles={ass}[v]"
    ));
    g
}

fn ass_time(ms: i64) -> String {
    let cs = ms.max(0) / 10;
    format!(
        "{}:{:02}:{:02}.{:02}",
        cs / 360_000,
        cs / 6000 % 60,
        cs / 100 % 60,
        cs % 100
    )
}

fn ass_text(s: &str) -> String {
    s.replace(['\n', '{', '}'], " ").replace('\\', "/")
}

fn speaker_name(speaker: &str, host: &ClipHost) -> String {
    match speaker {
        s if is_host(s) => host.name.to_uppercase(),
        "viewer" => host.viewer_asks(),
        foil => foil
            .trim_start_matches("the-")
            .replace('-', " ")
            .to_uppercase(),
    }
}

/// The ASS header shared by both cuts' tracks. Colours are the stage's caption colours
/// (apps/stage `--bone` #efe7d2 for the line, `--verdigris` #5fe0bf for the speaker, `--dim`
/// #8b938d for a line the voice never said), as `&HAABBGGRR`. Georgia is the stage caption
/// face's own fallback (`--display`: Cinzel, then Georgia); the line is set in capitals, as
/// Cinzel draws it.
fn ass_head(w: u32, h: u32, caption_top: u32) -> String {
    let line_top = caption_top + 56;
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Speaker,Georgia,34,&H00BFE05F,&H000000FF,&H00070806,&H99000000,1,0,0,0,100,100,10,0,1,2,0,7,72,72,{caption_top},1\n\
Style: Line,Georgia,56,&H00D2E7EF,&H000000FF,&H00070806,&H99000000,1,0,0,0,100,100,1,0,1,3,2,7,72,72,{line_top},1\n\
Style: Unheard,Georgia,50,&H008D938B,&H000000FF,&H00070806,&H99000000,0,1,0,0,100,100,1,0,1,3,2,7,72,72,{line_top},1\n\
Style: Header,Georgia,40,&H002B6AFF,&H000000FF,&H00070806,&HC0060807,1,0,0,0,100,100,6,0,3,10,0,8,40,40,20,1\n\
Style: Cite,Consolas,26,&H00D2E7EF,&H000000FF,&H00070806,&HC0060807,0,0,0,0,100,100,0,0,3,8,0,2,40,40,20,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    )
}

/// The ALLEGED header, grade badge and citation a SEG-02A cut carries, for `dur_ms`.
fn evidence_events(header: &ClipHeader, dur_ms: i64) -> String {
    let (a, b) = (ass_time(0), ass_time(dur_ms));
    let mut out = format!(
        "Dialogue: 1,{a},{b},Header,,0,0,0,,ALLEGED · {} · {}{}\n",
        ass_text(&header.grade),
        ass_text(&header.case_title),
        if header.fictional {
            " · FICTIONAL DEMO CASE"
        } else {
            ""
        }
    );
    let cite = if header.citation.is_empty() {
        "No hex claim in this cut · case shown by title".to_string()
    } else {
        format!(
            "SOURCES · {}",
            header
                .citation
                .iter()
                .take(3)
                .map(|c| c.trim_start_matches("https://").to_string())
                .collect::<Vec<_>>()
                .join(" · ")
        )
    };
    out.push_str(&format!(
        "Dialogue: 1,{a},{b},Cite,,0,0,0,,{}\n",
        ass_text(&cite)
    ));
    out
}

/// The vertical cut's captions: each line's speaker and text, in capitals, on the cut's clock
/// (the window's clock moved by `shift_ms`). An unheard line is set dim and italic, so the
/// viewer of the draft sees it was never said.
pub fn vertical_ass(
    captions: &[ClipCaption],
    rf: &Reframe,
    shift_ms: i64,
    dur_ms: i64,
    header: Option<&ClipHeader>,
    host: &ClipHost,
) -> String {
    let mut out = ass_head(V_WIDTH, V_HEIGHT, rf.caption_top);
    // One line on screen at a time: each ends where the next begins.
    let mut order: Vec<&ClipCaption> = captions.iter().collect();
    order.sort_by_key(|c| c.start_ms);
    for (k, c) in order.iter().enumerate() {
        let next = order.get(k + 1).map_or(i64::MAX, |n| n.start_ms);
        let end = c.end_ms.min(next).max(c.start_ms);
        if end + shift_ms <= 0 {
            continue;
        }
        let (a, b) = (
            ass_time(c.start_ms + shift_ms),
            ass_time((end + shift_ms).min(dur_ms)),
        );
        out.push_str(&format!(
            "Dialogue: 0,{a},{b},Speaker,,0,0,0,,{}\n",
            ass_text(&speaker_name(&c.speaker, host))
        ));
        let style = if c.unvoiced { "Unheard" } else { "Line" };
        out.push_str(&format!(
            "Dialogue: 0,{a},{b},{style},,0,0,0,,{}\n",
            ass_text(&c.text.to_uppercase())
        ));
    }
    if let Some(h) = header {
        out.push_str(&evidence_events(h, dur_ms));
    }
    out
}

/// The 16:9 cut's track: only a SEG-02A cut's evidence header (the stage's captions are
/// already in the picture).
pub fn landscape_ass(width: u32, height: u32, dur_ms: i64, header: &ClipHeader) -> String {
    let mut out = ass_head(width, height, 0);
    out.push_str(&evidence_events(header, dur_ms));
    out
}

// ----- encoding ---------------------------------------------------------------------------

const ENCODE: [&str; 12] = [
    "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac",
    "-b:a", "160k",
];

fn run_ffmpeg(ffmpeg: &str, cwd: &Path, args: &[String]) -> Result<(), String> {
    let out = quiet(&mut Command::new(ffmpeg))
        .current_dir(cwd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not start ffmpeg: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(4).collect();
        Err(format!(
            "ffmpeg failed ({}): {}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ))
    }
}

fn input_args(rec: &ClipRecording, s: Span) -> Vec<String> {
    vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-ss".into(),
        secs(s.in_ms),
        "-i".into(),
        rec.file.clone(),
        "-t".into(),
        secs(s.dur_ms),
    ]
}

fn output_args(fps: u32, out: &str) -> Vec<String> {
    let mut a: Vec<String> = vec!["-map".into(), "0:a:0?".into()];
    a.extend(ENCODE.iter().map(|s| s.to_string()));
    a.extend([
        "-r".into(),
        fps.to_string(),
        "-movflags".into(),
        "+faststart".into(),
        out.into(),
    ]);
    a
}

/// `cut-16x9.mp4` in `dir`: the span re-encoded (input seeking with a re-encode cuts on the
/// frame), plus a SEG-02A cut's evidence track from `cut-16x9.ass`.
pub fn cut_landscape(
    ffmpeg: &str,
    dir: &Path,
    rec: &ClipRecording,
    s: Span,
    evidence_ass: Option<&str>,
) -> Result<(), String> {
    let mut a = input_args(rec, s);
    match evidence_ass {
        Some(name) => a.extend([
            "-filter_complex".into(),
            format!("[0:v]subtitles={name}[v]"),
            "-map".into(),
            "[v]".into(),
        ]),
        None => a.extend(["-map".into(), "0:v:0".into()]),
    }
    a.extend(output_args(rec.fps, "cut-16x9.mp4"));
    run_ffmpeg(ffmpeg, dir, &a)
}

/// `cut-9x16.mp4` in `dir`, captions from `ass` (a file name in `dir`).
pub fn cut_vertical(
    ffmpeg: &str,
    dir: &Path,
    rec: &ClipRecording,
    s: Span,
    rf: &Reframe,
    ass: &str,
) -> Result<(), String> {
    let mut a = input_args(rec, s);
    a.extend([
        "-filter_complex".into(),
        vertical_filter(rf, rec.fps, s.dur_ms, ass),
        "-map".into(),
        "[v]".into(),
    ]);
    a.extend(output_args(rec.fps, "cut-9x16.mp4"));
    run_ffmpeg(ffmpeg, dir, &a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> ClipRecording {
        ClipRecording {
            file: "recording.mp4".into(),
            stream_offset_ms: -542,
            width: 1920,
            height: 1080,
            fps: 30,
            av_offset_ms: AV_OFFSET_MS,
        }
    }

    fn window(start: i64, end: i64, band_max: u32) -> ClipWindow {
        ClipWindow {
            anchor_ms: start,
            start_ms: start,
            end_ms: end,
            spoken_start_ms: start,
            spoken_end_ms: end,
            duration_s: (end - start) as f64 / 1000.0,
            driver: "marker".into(),
            lag_ms: 0,
            preroll_ms: 0,
            band_s: [15, band_max],
            style: "default".into(),
            needs_reauthoring: false,
            note: None,
            voice_timed: true,
        }
    }

    #[test]
    fn a_stream_time_sits_at_t_minus_the_offset() {
        // The guide's example: played at 10720 with an offset of -542 is 11.262 s in.
        let s = span(&rec(), &window(10_720, 40_720, 60));
        assert_eq!(
            s,
            Span {
                in_ms: 11_262,
                dur_ms: 30_000,
                shift_ms: 0
            }
        );
        // Never before the file's first frame: the lost head shifts the captions.
        let s = span(&rec(), &window(-1_000, 20_000, 60));
        assert_eq!(
            s,
            Span {
                in_ms: 0,
                dur_ms: 21_000 - 458,
                shift_ms: -458
            }
        );
    }

    #[test]
    fn the_overall_rms_level_is_read() {
        let err = "[Parsed_astats_0 @ 0] Overall\n[Parsed_astats_0 @ 0] Peak level dB: -3.206050\n[Parsed_astats_0 @ 0] RMS level dB: -24.921683\n[Parsed_astats_0 @ 0] RMS peak dB: -16.413837\n";
        assert_eq!(parse_rms(err), Some(-24.921683));
        assert_eq!(parse_rms("[x] RMS level dB: -inf\n"), Some(FLOOR_DB));
        assert_eq!(parse_rms("no report"), None);
    }

    #[test]
    fn loudness_maps_onto_the_signal_around_the_streams_own_level() {
        assert_eq!(audio_value(-22.0, -22.0), 0.5);
        assert_eq!(audio_value(-16.0, -22.0), 1.0);
        assert_eq!(audio_value(-30.0, -22.0), 0.0);
        assert_eq!(audio_value(-70.0, -22.0), 0.0);
        assert_eq!(audio_value(-19.0, -22.0), 0.75);
    }

    #[test]
    fn each_layout_reframes_its_own_region() {
        let improv = reframe_for(SegmentId::Seg01ImprovisedRoom);
        assert_eq!(improv.layout, "improv");
        assert_eq!(improv.main, IMPROV_FACE);
        // 560x612 at 1080 wide is 1180 tall; captions sit below it.
        assert_eq!(improv.caption_top, 170 + 1180 + 40);
        let g = vertical_filter(&improv, 30, 30_400, "captions-9x16.ass");
        assert!(g.contains("crop=560:612:64:112,scale=1080:-2"), "{g}");
        assert!(g.contains("crop=320:52:1584:30,scale=480:-2"), "{g}");
        assert!(g.contains("d=30.400"), "{g}");
        assert!(g.ends_with("[t3]subtitles=captions-9x16.ass[v]"), "{g}");
        assert!(!g.contains("[inset]"));
        let hex = reframe_for(SegmentId::Seg02aHexmap);
        assert_eq!(hex.layout, "hexmap");
        let g = vertical_filter(&hex, 30, 50_000, "c.ass");
        assert!(g.contains("split=3"), "{g}");
        assert!(g.contains("crop=448:448:16:632,scale=420:420"), "{g}");
        assert!(hex.caption_top + 400 < V_HEIGHT);
    }

    #[test]
    fn the_rects_match_the_stage_layouts() {
        let layouts = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/stage/src/layouts.ts"),
        )
        .expect("apps/stage/src/layouts.ts");
        for want in [
            "const TOP = 104;",
            "export const FACE_PX = 560;",
            "const HOST_AVATAR: Rect = { x: 64, y: TOP + 8, w: FACE_PX, h: 612 };",
            "export const CORNER_AVATAR: Rect = { x: 16, y: 1080 - 448, w: 448, h: 448 };",
            "export const GUTTER = 32;",
            "const CONTENT: Rect = { x: GUTTER, y: TOP, w: RIGHT_X - GUTTER - 24, h: 804 };",
            "const RIGHT_X = 1312;",
        ] {
            assert!(layouts.contains(want), "layouts.ts changed: {want}");
        }
        assert_eq!(CONTENT_SCREEN.w, 1312 - 32 - 24);
        assert_eq!(CORNER_FACE.y, 1080 - 448);
    }

    #[test]
    fn captions_burn_in_capitals_on_the_cuts_clock_and_unheard_lines_dim() {
        let caps = vec![
            ClipCaption {
                start_ms: 0,
                end_ms: 2_000,
                speaker: "malakor".into(),
                text: "The throne {is} rented.".into(),
                unvoiced: false,
            },
            ClipCaption {
                start_ms: 2_500,
                end_ms: 4_000,
                speaker: "the-auditor".into(),
                text: "Objection.".into(),
                unvoiced: true,
            },
        ];
        let rf = reframe_for(SegmentId::Seg01ImprovisedRoom);
        let host = ClipHost {
            full_name: "Arch-Lich Malakor".into(),
            name: "Malakor".into(),
            follower: "minion".into(),
        };
        let a = vertical_ass(&caps, &rf, 0, 3_800, None, &host);
        assert!(a.contains("PlayResX: 1080\nPlayResY: 1920"));
        assert!(a.contains("Dialogue: 0,0:00:00.00,0:00:02.00,Speaker,,0,0,0,,MALAKOR"));
        assert!(
            a.contains("Dialogue: 0,0:00:00.00,0:00:02.00,Line,,0,0,0,,THE THRONE  IS  RENTED.")
        );
        assert!(a.contains("Dialogue: 0,0:00:02.50,0:00:03.80,Unheard,,0,0,0,,OBJECTION."));
        assert!(a.contains(",0:00:02.50,0:00:03.80,Speaker,,0,0,0,,AUDITOR"));
        // A head lost before the recording began moves every line earlier; a line wholly
        // before the cut is dropped.
        let a = vertical_ass(&caps, &rf, -2_200, 1_600, None, &host);
        assert!(!a.contains("MALAKOR"));
        assert!(a.contains("Dialogue: 0,0:00:00.30,0:00:01.60,Unheard"));
        assert!(!a.contains("ALLEGED"));
        let h = ClipHeader {
            alleged: true,
            grade: "CLM".into(),
            case_title: "The Tray".into(),
            fictional: true,
            citation: vec!["https://example.org/a".into()],
        };
        let a = vertical_ass(&caps, &rf, 0, 4_000, Some(&h), &host);
        assert!(a.contains("Header,,0,0,0,,ALLEGED · CLM · The Tray · FICTIONAL DEMO CASE"));
        assert!(a.contains("Cite,,0,0,0,,SOURCES · example.org/a"));
        let l = landscape_ass(1920, 1080, 4_000, &h);
        assert!(l.contains("PlayResX: 1920\nPlayResY: 1080"));
        assert!(l.contains("ALLEGED · CLM"));
    }

    /// A synthetic WAV: `secs` of a 440 Hz tone at `amp` (full scale 1.0), mono 48 kHz.
    fn wav(path: &Path, secs: f64, amp: f64) {
        let rate = 48_000u32;
        let n = (secs * rate as f64) as u32;
        let mut b: Vec<u8> = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + n * 2).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(rate.to_le_bytes());
        b.extend((rate * 2).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend((n * 2).to_le_bytes());
        for i in 0..n {
            let t = i as f64 / rate as f64;
            let v = (amp * (2.0 * std::f64::consts::PI * 440.0 * t).sin() * 32767.0) as i16;
            b.extend(v.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    fn ffmpeg_present() -> bool {
        quiet(&mut Command::new("ffmpeg"))
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// The measurement against a synthetic signal: a sine at half scale is -9.03 dBFS RMS, at
    /// quarter scale 6.02 dB lower; a window holds only its own part of the file; silence in
    /// a window pulls its level down (ungated). Needs ffmpeg on PATH; without it the test
    /// says so and passes (CI has no media tools).
    #[test]
    fn level_is_measured_on_a_synthetic_wav() {
        if !ffmpeg_present() {
            eprintln!("skipped: ffmpeg is not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let loud = dir.path().join("loud.wav");
        let soft = dir.path().join("soft.wav");
        wav(&loud, 4.0, 0.5);
        wav(&soft, 4.0, 0.25);
        let l = rms_db("ffmpeg", &loud, None).unwrap();
        let s = rms_db("ffmpeg", &soft, None).unwrap();
        assert!((l - -9.03).abs() < 0.1, "loud {l}");
        assert!(((l - s) - 6.02).abs() < 0.1, "loud {l} soft {s}");
        let w = rms_db("ffmpeg", &soft, Some((1_000, 2_000))).unwrap();
        assert!((w - s).abs() < 0.1, "window {w} whole {s}");
        assert!(audio_value(l, s) > 0.99);
        // Half the window silent: 3 dB down.
        let gap = dir.path().join("gap.wav");
        wav(&gap, 4.0, 0.0);
        let mut both = std::fs::read(&loud).unwrap();
        let silence = std::fs::read(&gap).unwrap();
        both.extend_from_slice(&silence[44..]);
        let n = (both.len() - 44) as u32;
        both[40..44].copy_from_slice(&n.to_le_bytes());
        both[4..8].copy_from_slice(&(36 + n).to_le_bytes());
        std::fs::write(&gap, both).unwrap();
        let g = rms_db("ffmpeg", &gap, None).unwrap();
        assert!(((l - g) - 3.01).abs() < 0.1, "loud {l} half silent {g}");
    }

    use crate::tests::{fake_ffmpeg, FakeFfmpeg};

    #[test]
    fn the_level_read_takes_a_window_and_reports_a_failed_or_missing_ffmpeg() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("programme.wav");
        std::fs::write(&file, b"not media: the stand-in never reads it").unwrap();
        let levels = fake_ffmpeg(dir.path(), FakeFfmpeg::Levels);
        // The stand-in reads -30 dB for the whole file and -10 dB for any seeked window.
        assert_eq!(rms_db(&levels, &file, None), Ok(-30.0));
        assert_eq!(rms_db(&levels, &file, Some((1_000, 2_000))), Ok(-10.0));
        assert_eq!(rms_db(&levels, &file, Some((-500, 0))), Ok(-10.0));
        // An ffmpeg that exits non-zero names its last lines.
        let fails = fake_ffmpeg(dir.path(), FakeFfmpeg::Fails);
        let e = rms_db(&fails, &file, None).unwrap_err();
        assert!(
            e.contains("ffmpeg astats failed") && e.contains("boom"),
            "{e}"
        );
        // One that succeeds without an RMS line has no audio to measure.
        let silent = fake_ffmpeg(dir.path(), FakeFfmpeg::Silent);
        let e = rms_db(&silent, &file, None).unwrap_err();
        assert!(e.contains("no RMS level"), "{e}");
        // One that is not there cannot start.
        let e = rms_db("no-such-ffmpeg-binary", &file, None).unwrap_err();
        assert!(e.starts_with("ffmpeg: "), "{e}");
    }

    #[test]
    fn a_cut_runs_ffmpeg_in_the_run_directory_and_reports_its_failure() {
        let dir = tempfile::tempdir().unwrap();
        let span = Span {
            in_ms: 12_000,
            dur_ms: 30_400,
            shift_ms: 0,
        };
        let rf = reframe_for(SegmentId::Seg01ImprovisedRoom);
        let ok = fake_ffmpeg(dir.path(), FakeFfmpeg::Silent);
        assert_eq!(cut_landscape(&ok, dir.path(), &rec(), span, None), Ok(()));
        assert_eq!(
            cut_landscape(&ok, dir.path(), &rec(), span, Some("captions.ass")),
            Ok(())
        );
        assert_eq!(
            cut_vertical(&ok, dir.path(), &rec(), span, &rf, "captions-9x16.ass"),
            Ok(())
        );
        let bad = fake_ffmpeg(dir.path(), FakeFfmpeg::Fails);
        let e = cut_landscape(&bad, dir.path(), &rec(), span, None).unwrap_err();
        assert!(e.contains("ffmpeg failed") && e.contains("boom"), "{e}");
        let e = cut_vertical(&bad, dir.path(), &rec(), span, &rf, "captions-9x16.ass").unwrap_err();
        assert!(e.contains("ffmpeg failed"), "{e}");
        let e = cut_landscape("no-such-ffmpeg-binary", dir.path(), &rec(), span, None).unwrap_err();
        assert!(e.starts_with("could not start ffmpeg"), "{e}");
    }

    #[test]
    fn the_recording_is_found_beside_the_run_or_where_the_json_points() {
        let dir = tempfile::tempdir().unwrap();
        // No recording.json, or one that does not parse: no recording.
        assert!(read_recording(dir.path()).is_none());
        std::fs::write(dir.path().join("recording.json"), "{ torn").unwrap();
        assert!(read_recording(dir.path()).is_none());
        // It names a file that is not there, and none sits beside it.
        let elsewhere = dir.path().join("elsewhere.mkv");
        let json = |file: &Path| {
            serde_json::json!({"file": file.to_string_lossy(), "stream_offset_ms": 1_500,
                "width": 1920, "height": 1080, "fps": 30})
            .to_string()
        };
        std::fs::write(dir.path().join("recording.json"), json(&elsewhere)).unwrap();
        assert!(read_recording(dir.path()).is_none());
        // The recorded path, once the file exists.
        std::fs::write(&elsewhere, b"x").unwrap();
        let r = read_recording(dir.path()).unwrap();
        assert_eq!(Path::new(&r.file), elsewhere);
        assert_eq!(
            (r.stream_offset_ms, r.width, r.height, r.fps),
            (1_500, 1920, 1080, 30)
        );
        assert_eq!(r.av_offset_ms, AV_OFFSET_MS);
        // The run directory's own recording wins over the recorded path.
        let own = dir.path().join("recording.mp4");
        std::fs::write(&own, b"x").unwrap();
        assert_eq!(Path::new(&read_recording(dir.path()).unwrap().file), own);
    }
}
