//! The vertical 1080x1920 draft.
//!
//! Every draft writes, into `<data>/clips/<id>/`:
//! - `clip.json`: the stage's input for the cut (its captions, the scene events of the window
//!   on the clip's own clock, the header and citation). `apps/stage` replays it in clip mode;
//! - `captions.srt` and `captions.ass`: the exact transcript lines (viewer handles redacted),
//!   timed by the stage's caption model;
//! - `poster.svg`: a still poster frame drawn here, with the same header, badge, citation and
//!   AI disclosure the video carries;
//! - `manifest.json`: what was rendered, from what, and what is burnt in.
//!
//! A run with a show recording is cut from it (`footage`: `cut-16x9.mp4` and the vertical
//! `cut-9x16.mp4`, plus a `poster.png` frame of the vertical). A run without one is re-drawn:
//! `render_video` adds `draft.mp4` (and a `poster.png` frame) by recording the stage in a
//! headless Chromium through the DevTools protocol and encoding the frames with ffmpeg
//! (`apps/stage/scripts/record-clip.mjs`), and the manifest labels it "re-drawn (no
//! recording)". When a tool is missing the draft stays a poster frame plus caption tracks, and
//! the manifest says which tool and why. A video is never faked: every file is checked with
//! ffprobe before it counts.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use shadowrunr_types::clips::{
    ClipCandidate, ClipCaption, ClipEvidence, ClipFile, ClipFootage, ClipOverlay, ClipRecording,
    ClipRender,
};
use shadowrunr_types::{Case, EvidenceGrade, StreamEvent};

use crate::footage;
use crate::host::{is_host, ClipHost, HOST_SPEAKER};
use crate::store::{write_bytes, write_json};
use crate::{clock, now_ms, ClipError, Result, HEIGHT, WIDTH};

pub const DISCLOSURE: &str = "AI character · synthetic media";
pub const CAPTION_SOURCE: &str = "transcript text (viewer handles redacted), timed by the stage caption model (~150 wpm); no TTS alignment in the media-less phase";
/// The captions of a cut from the recording.
pub const CAPTION_SOURCE_VOICE: &str = "transcript text (viewer handles redacted), timed by the voice's real playback (voice.jsonl); the 16:9 cut shows the stage's own captions, the 9:16 burns these in";
/// The manifest's label for each footage.
pub const FOOTAGE_RECORDING: &str = "cut from the recording";
pub const FOOTAGE_REDRAWN: &str = "re-drawn (no recording)";
const FPS: u32 = 30;

/// The header a SEG-02A or case-linked cut carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipHeader {
    pub alleged: bool,
    /// `DOC`, `REP`, `CLM`, `SPC` or `UNGRADED`.
    pub grade: String,
    pub case_title: String,
    pub fictional: bool,
    pub citation: Vec<String>,
}

/// `clip.json`, the stage's input (`apps/stage/src/clip.ts` reads it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInput {
    pub version: u32,
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: i64,
    pub style: String,
    pub segment: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<ClipHeader>,
    pub captions: Vec<ClipCaption>,
    /// Scene events on the clip's clock (0 = the cut's first frame); the state before the
    /// cut is replayed at 0. Never chat: handles do not leave the room.
    pub events: Vec<StreamEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<Case>,
    pub disclosure: String,
    pub provenance: String,
    /// Who the host is (the run's character); a draft written before it was recorded reads
    /// the neutral host.
    #[serde(default)]
    pub host: ClipHost,
}

pub fn grade_name(g: Option<EvidenceGrade>) -> String {
    match g {
        Some(EvidenceGrade::Doc) => "DOC",
        Some(EvidenceGrade::Rep) => "REP",
        Some(EvidenceGrade::Clm) => "CLM",
        Some(EvidenceGrade::Spc) => "SPC",
        None => "UNGRADED",
    }
    .to_string()
}

pub fn header_for(evidence: Option<&ClipEvidence>) -> Option<ClipHeader> {
    evidence.map(|e| ClipHeader {
        alleged: true,
        grade: grade_name(e.worst_grade),
        case_title: e.case_title.clone(),
        fictional: e.fictional,
        citation: e.citation.clone(),
    })
}

/// The overlay the draft burns in.
pub fn overlay_for(header: Option<&ClipHeader>) -> ClipOverlay {
    ClipOverlay {
        alleged_header: header.is_some_and(|h| h.alleged),
        grade_badge: header.map(|h| h.grade.clone()),
        citation: header.map(|h| h.citation.clone()).unwrap_or_default(),
        ai_disclosure: true,
        captions: CAPTION_SOURCE.to_string(),
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Greedy word wrap at `width` characters, at most `max` lines (the last one ellipsised).
pub fn wrap(text: &str, width: usize, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for w in text.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(w);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.len() > max {
        lines.truncate(max);
        if let Some(l) = lines.last_mut() {
            l.push('…');
        }
    }
    lines
}

fn grade_color(g: &str) -> &'static str {
    match g {
        "DOC" => "#86d3bb",
        "REP" => "#cdbf9f",
        "CLM" => "#e8913f",
        "SPC" => "#ff5a2a",
        _ => "#9a9284",
    }
}

/// The poster frame: the payoff line large, its setup above it, the header, badge, citation
/// and disclosure exactly as the video draws them.
pub fn poster_svg(input: &ClipInput, payoff: &ClipCaption, setup: Option<&ClipCaption>) -> String {
    let mut s = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
<rect width="100%" height="100%" fill="#0d1312"/>
<rect x="0" y="0" width="{WIDTH}" height="8" fill="#ff6a2b"/>
"##
    );
    let mut y = 120;
    if let Some(h) = &input.header {
        s.push_str(&format!(
            r##"<text x="72" y="{y}" font-family="Cinzel, Georgia, serif" font-size="96" font-weight="700" fill="#ff6a2b" letter-spacing="8">ALLEGED</text>
<rect x="760" y="{}" width="248" height="104" rx="10" fill="{}"/>
<text x="884" y="{}" text-anchor="middle" font-family="IBM Plex Mono, Consolas, monospace" font-size="56" font-weight="700" fill="#0b0f0e">{}</text>
"##,
            y - 84,
            grade_color(&h.grade),
            y - 12,
            xml(&h.grade)
        ));
        y += 70;
        for l in wrap(&h.case_title, 40, 2) {
            s.push_str(&format!(
                r##"<text x="72" y="{y}" font-family="IBM Plex Sans Condensed, Arial Narrow, sans-serif" font-size="40" fill="#e5dcc7">{}</text>
"##,
                xml(&l)
            ));
            y += 50;
        }
        if h.fictional {
            s.push_str(&format!(
                r##"<rect x="72" y="{}" width="420" height="52" rx="6" fill="none" stroke="#e8913f" stroke-width="3"/>
<text x="92" y="{}" font-family="IBM Plex Mono, Consolas, monospace" font-size="30" fill="#e8913f">FICTIONAL DEMO CASE</text>
"##,
                y - 10,
                y + 26
            ));
            y += 70;
        }
    } else {
        s.push_str(&format!(
            r##"<text x="72" y="{y}" font-family="Cinzel, Georgia, serif" font-size="64" font-weight="700" fill="#86d3bb" letter-spacing="4">{}</text>
"##,
            xml(&input.title.chars().take(28).collect::<String>())
        ));
        y += 40;
    }
    let mut ty = (y + 260).max(560);
    if let Some(st) = setup {
        s.push_str(&format!(
            r##"<text x="72" y="{ty}" font-family="IBM Plex Mono, Consolas, monospace" font-size="30" fill="#8f877a">{}</text>
"##,
            xml(&speaker_title(&st.speaker, &input.host))
        ));
        ty += 56;
        for l in wrap(&st.text, 44, 4) {
            s.push_str(&format!(
                r##"<text x="72" y="{ty}" font-family="IBM Plex Sans Condensed, Arial Narrow, sans-serif" font-size="42" fill="#cdbf9f">{}</text>
"##,
                xml(&l)
            ));
            ty += 54;
        }
        ty += 60;
    }
    s.push_str(&format!(
        r##"<text x="72" y="{ty}" font-family="IBM Plex Mono, Consolas, monospace" font-size="32" fill="#ff6a2b">{}</text>
"##,
        xml(&speaker_title(&payoff.speaker, &input.host))
    ));
    ty += 84;
    // Never into the citation strip (it starts at 1600).
    let room = ((1_600 - ty) / 86).clamp(2, 9) as usize;
    for l in wrap(&payoff.text, 26, room) {
        s.push_str(&format!(
            r##"<text x="72" y="{ty}" font-family="Cinzel, Georgia, serif" font-size="68" font-weight="700" fill="#efe6d2">{}</text>
"##,
            xml(&l)
        ));
        ty += 86;
    }
    let mut cy = 1700;
    if let Some(h) = &input.header {
        if h.citation.is_empty() {
            s.push_str(&format!(
                r##"<text x="72" y="{cy}" font-family="IBM Plex Mono, Consolas, monospace" font-size="28" fill="#cdbf9f">No hex claim in this cut · case shown by title</text>
"##
            ));
        } else {
            s.push_str(&format!(
                r##"<text x="72" y="{}" font-family="IBM Plex Mono, Consolas, monospace" font-size="26" fill="#8f877a">SOURCES · grade {}</text>
"##,
                cy - 44,
                xml(&h.grade)
            ));
            for c in h.citation.iter().take(3) {
                let short: String = c.trim_start_matches("https://").chars().take(56).collect();
                s.push_str(&format!(
                    r##"<text x="72" y="{cy}" font-family="IBM Plex Mono, Consolas, monospace" font-size="28" fill="#cdbf9f">{}</text>
"##,
                    xml(&short)
                ));
                cy += 40;
            }
        }
    }
    s.push_str(&format!(
        r##"<rect x="72" y="1812" width="520" height="56" rx="28" fill="#161411" stroke="#86d3bb" stroke-width="2"/>
<text x="100" y="1850" font-family="IBM Plex Mono, Consolas, monospace" font-size="28" fill="#86d3bb">{}</text>
<text x="1008" y="1850" text-anchor="end" font-family="IBM Plex Mono, Consolas, monospace" font-size="22" fill="#6b5f4a">{}</text>
</svg>
"##,
        xml(DISCLOSURE),
        xml(&input.provenance.chars().take(48).collect::<String>())
    ));
    s
}

fn speaker_title(speaker: &str, host: &ClipHost) -> String {
    match speaker {
        s if is_host(s) => host.full_name.to_uppercase(),
        "viewer" => host.viewer_asks(),
        foil => foil
            .trim_start_matches("the-")
            .replace('-', " ")
            .to_uppercase(),
    }
}

fn srt_time(ms: i64) -> String {
    let ms = ms.max(0);
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
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

pub fn srt(captions: &[ClipCaption], host: &ClipHost) -> String {
    let mut out = String::new();
    for (i, c) in captions.iter().enumerate() {
        let who = if is_host(&c.speaker) {
            String::new()
        } else {
            format!("[{}] ", speaker_title(&c.speaker, host))
        };
        out.push_str(&format!(
            "{}\n{} --> {}\n{}{}\n\n",
            i + 1,
            srt_time(c.start_ms),
            srt_time(c.end_ms),
            who,
            c.text.replace('\n', " ")
        ));
    }
    out
}

pub fn ass(captions: &[ClipCaption], host: &ClipHost) -> String {
    let host_style = host.style();
    let mut out = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {WIDTH}\nPlayResY: {HEIGHT}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: {host_style},IBM Plex Sans Condensed,64,&H00D2E6EF,&H000000FF,&H00120F0B,&H96000000,1,0,0,0,100,100,0,0,1,4,0,2,72,72,360,1\n\
Style: Other,IBM Plex Sans Condensed,56,&H00A5BFCD,&H000000FF,&H00120F0B,&H96000000,0,1,0,0,100,100,0,0,1,4,0,2,72,72,360,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    );
    for c in captions {
        let style = if is_host(&c.speaker) {
            host_style.as_str()
        } else {
            "Other"
        };
        let text = c.text.replace(['\n', '{', '}'], " ");
        out.push_str(&format!(
            "Dialogue: 0,{},{},{style},{},0,0,0,,{}\n",
            ass_time(c.start_ms),
            ass_time(c.end_ms),
            speaker_title(&c.speaker, host),
            text
        ));
    }
    out
}

fn file_entry(dir: &Path, rel: &str, kind: &str) -> ClipFile {
    ClipFile {
        path: rel.to_string(),
        kind: kind.to_string(),
        bytes: std::fs::metadata(dir.join(rel))
            .map(|m| m.len())
            .unwrap_or(0),
    }
}

/// Write the draft's poster, caption tracks, stage input and manifest; returns the render
/// record (kind `poster`, with `fallback_reason` saying why there is no video yet).
pub fn write_draft(
    dir: &Path,
    c: &ClipCandidate,
    input: &ClipInput,
    fallback_reason: &str,
) -> Result<ClipRender> {
    let payoff = payoff_caption(&input.captions);
    let setup = payoff.and_then(|p| {
        input
            .captions
            .iter()
            .rev()
            .find(|x| x.start_ms < p.start_ms)
    });
    let fallback = ClipCaption {
        start_ms: 0,
        end_ms: input.duration_ms,
        speaker: HOST_SPEAKER.into(),
        text: c.title.clone(),
        unvoiced: false,
    };
    write_json(&dir.join("clip.json"), input)?;
    write_bytes(
        &dir.join("captions.srt"),
        srt(&input.captions, &input.host).as_bytes(),
    )?;
    write_bytes(
        &dir.join("captions.ass"),
        ass(&input.captions, &input.host).as_bytes(),
    )?;
    write_bytes(
        &dir.join("poster.svg"),
        poster_svg(input, payoff.unwrap_or(&fallback), setup).as_bytes(),
    )?;
    let render = ClipRender {
        kind: "poster".into(),
        footage: Some(if c.recording.is_some() {
            ClipFootage::Recording
        } else {
            ClipFootage::Redrawn
        }),
        width: WIDTH,
        height: HEIGHT,
        duration_s: input.duration_ms as f64 / 1000.0,
        files: vec![
            file_entry(dir, "poster.svg", "poster"),
            file_entry(dir, "captions.srt", "captions_srt"),
            file_entry(dir, "captions.ass", "captions_ass"),
            file_entry(dir, "clip.json", "stage_input"),
        ],
        overlay: overlay_for(input.header.as_ref()),
        fallback_reason: Some(fallback_reason.to_string()),
        rendered_ms: now_ms(),
    };
    write_manifest(dir, c, &render)?;
    Ok(render)
}

/// The payoff caption: the longest host line, else the last caption.
fn payoff_caption(captions: &[ClipCaption]) -> Option<&ClipCaption> {
    captions
        .iter()
        .filter(|c| is_host(&c.speaker))
        .max_by_key(|c| c.text.len())
        .or_else(|| captions.last())
}

#[derive(Serialize)]
struct Manifest<'a> {
    id: &'a str,
    /// "cut from the recording" or "re-drawn (no recording)".
    footage: &'static str,
    render: &'a ClipRender,
    window: &'a shadowrunr_types::clips::ClipWindow,
    source: &'a shadowrunr_types::clips::ClipSource,
    stream_clock: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording: Option<&'a ClipRecording>,
    /// Where the cut sits in the recording.
    #[serde(skip_serializing_if = "Option::is_none")]
    file_span: Option<FileSpan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    av_offset_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    av_offset_provenance: Option<&'static str>,
}

#[derive(Serialize)]
struct FileSpan {
    in_ms: i64,
    dur_ms: i64,
    #[serde(skip_serializing_if = "is_zero")]
    shift_ms: i64,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

fn write_manifest(dir: &Path, c: &ClipCandidate, r: &ClipRender) -> Result<()> {
    write_manifest_with(dir, c, r, None)
}

fn write_manifest_with(
    dir: &Path,
    c: &ClipCandidate,
    r: &ClipRender,
    cut: Option<(&ClipRecording, footage::Span)>,
) -> Result<()> {
    write_json(
        &dir.join("manifest.json"),
        &Manifest {
            id: &c.id,
            footage: if r.footage == Some(ClipFootage::Recording) {
                FOOTAGE_RECORDING
            } else {
                FOOTAGE_REDRAWN
            },
            render: r,
            window: &c.window,
            source: &c.source,
            stream_clock: format!("{}-{}", clock(c.window.start_ms), clock(c.window.end_ms)),
            recording: cut.map(|(rec, _)| rec),
            file_span: cut.map(|(_, s)| FileSpan {
                in_ms: s.in_ms,
                dur_ms: s.dur_ms,
                shift_ms: s.shift_ms,
            }),
            av_offset_ms: cut.map(|(rec, _)| rec.av_offset_ms),
            av_offset_provenance: cut.map(|_| footage::AV_OFFSET_PROVENANCE),
        },
    )
}

// ----- video ------------------------------------------------------------------------------

/// Where the recording tools are. `chrome: None` searches the usual install locations.
#[derive(Debug, Clone)]
pub struct RenderTools {
    /// `apps/stage` (its `dist/` build and `scripts/record-clip.mjs`).
    pub stage_dir: PathBuf,
    pub node: String,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub chrome: Option<PathBuf>,
}

/// What the probe found: the resolved tools, or every missing piece.
#[derive(Debug, Clone)]
pub struct ToolProbe {
    pub chrome: Option<PathBuf>,
    pub missing: Vec<String>,
}

impl ToolProbe {
    pub fn ready(&self) -> bool {
        self.missing.is_empty()
    }
}

pub(crate) fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a spawned tool never flashes a console window.
        cmd.creation_flags(0x0800_0000);
    }
    cmd.stdin(Stdio::null())
}

fn runs(program: &str, arg: &str) -> bool {
    quiet(Command::new(program).arg(arg))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Headless-capable Chromium builds in their usual places, newest Playwright build last.
fn chrome_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        for base in [
            env("ProgramFiles"),
            env("ProgramFiles(x86)"),
            env("LOCALAPPDATA"),
        ]
        .into_iter()
        .flatten()
        {
            out.push(base.join("Google/Chrome/Application/chrome.exe"));
            out.push(base.join("Microsoft/Edge/Application/msedge.exe"));
        }
        if let Some(local) = env("LOCALAPPDATA") {
            let pw = local.join("ms-playwright");
            let mut builds: Vec<PathBuf> = std::fs::read_dir(&pw)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .map(|e| e.path())
                        .filter(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with("chromium-"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            builds.sort();
            for b in builds.into_iter().rev() {
                out.push(b.join("chrome-win/chrome.exe"));
                out.push(b.join("chrome-win64/chrome.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        out.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
        out.push("/Applications/Chromium.app/Contents/MacOS/Chromium".into());
    } else if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["google-chrome", "chromium", "chromium-browser"] {
                out.push(dir.join(name));
            }
        }
    }
    out
}

/// Check every recording tool; never fails, only reports.
pub fn probe_tools(t: &RenderTools) -> ToolProbe {
    let mut missing = Vec::new();
    if !t.stage_dir.join("dist").join("index.html").is_file() {
        missing.push(format!(
            "the stage build ({}/dist; run `pnpm -C apps/stage build`)",
            t.stage_dir.display()
        ));
    }
    if !t
        .stage_dir
        .join("scripts")
        .join("record-clip.mjs")
        .is_file()
    {
        missing.push(format!(
            "the recorder ({}/scripts/record-clip.mjs)",
            t.stage_dir.display()
        ));
    }
    if !runs(&t.node, "--version") {
        missing.push(format!("node ({})", t.node));
    }
    if !runs(&t.ffmpeg, "-version") {
        missing.push(format!("ffmpeg ({})", t.ffmpeg));
    }
    if !runs(&t.ffprobe, "-version") {
        missing.push(format!("ffprobe ({})", t.ffprobe));
    }
    let chrome = match &t.chrome {
        Some(p) if p.is_file() => Some(p.clone()),
        Some(p) => {
            missing.push(format!(
                "a headless Chromium ({} does not exist)",
                p.display()
            ));
            None
        }
        None => {
            let found = chrome_candidates().into_iter().find(|p| p.is_file());
            if found.is_none() {
                missing.push(
                    "a headless Chromium (Chrome, Edge or a Playwright chromium build)".into(),
                );
            }
            found
        }
    };
    ToolProbe { chrome, missing }
}

#[derive(Deserialize)]
struct Probe {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Deserialize)]
struct ProbeStream {
    width: Option<u32>,
    height: Option<u32>,
    codec_name: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// `(width, height, codec, duration_s)` of the first video stream, by ffprobe.
pub fn ffprobe(ffprobe: &str, file: &Path) -> std::result::Result<(u32, u32, String, f64), String> {
    let out = quiet(&mut Command::new(ffprobe))
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name:format=duration",
            "-of",
            "json",
        ])
        .arg(file)
        .output()
        .map_err(|e| format!("ffprobe: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ffprobe: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let p: Probe =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("ffprobe output: {e}"))?;
    let s = p.streams.first().ok_or("ffprobe found no video stream")?;
    let duration = p
        .format
        .and_then(|f| f.duration)
        .and_then(|d| d.parse::<f64>().ok())
        .unwrap_or(0.0);
    Ok((
        s.width.unwrap_or(0),
        s.height.unwrap_or(0),
        s.codec_name.clone().unwrap_or_default(),
        duration,
    ))
}

/// Record `clip.json` to `draft.mp4` with the stage, then verify it. `Err` carries the
/// reason the draft stays a poster.
pub fn render_video(
    dir: &Path,
    t: &RenderTools,
    probe: &ToolProbe,
    duration_ms: i64,
) -> std::result::Result<(f64, String), String> {
    if !probe.ready() {
        return Err(format!(
            "recording tools missing: {}",
            probe.missing.join("; ")
        ));
    }
    let chrome = probe.chrome.as_ref().ok_or("no Chromium")?;
    let out = dir.join("draft.mp4");
    let _ = std::fs::remove_file(&out);
    let result = quiet(&mut Command::new(&t.node))
        .arg(t.stage_dir.join("scripts").join("record-clip.mjs"))
        .arg("--dist")
        .arg(t.stage_dir.join("dist"))
        .arg("--clip")
        .arg(dir.join("clip.json"))
        .arg("--out")
        .arg(&out)
        .arg("--poster")
        .arg(dir.join("poster.png"))
        .arg("--chrome")
        .arg(chrome)
        .arg("--ffmpeg")
        .arg(&t.ffmpeg)
        .arg("--fps")
        .arg(FPS.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("could not start the recorder: {e}"))?;
    if !result.status.success() {
        let err = String::from_utf8_lossy(&result.stderr);
        let tail: Vec<&str> = err.lines().rev().take(6).collect();
        return Err(format!(
            "the recorder failed ({}): {}",
            result.status,
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ));
    }
    let (w, h, codec, dur) = ffprobe(&t.ffprobe, &out)?;
    if (w, h) != (WIDTH, HEIGHT) {
        return Err(format!("the recording is {w}x{h}, not {WIDTH}x{HEIGHT}"));
    }
    let want = duration_ms as f64 / 1000.0;
    if (dur - want).abs() > 1.0 {
        return Err(format!("the recording runs {dur:.2} s, not {want:.2} s"));
    }
    Ok((dur, codec))
}

/// Cut both pictures from the show's recording and verify them. `Err` carries the reason
/// the draft stays a poster.
fn cut_from_recording(
    dir: &Path,
    c: &ClipCandidate,
    t: &RenderTools,
    rec: &ClipRecording,
    host: &ClipHost,
) -> std::result::Result<(ClipRender, footage::Span), String> {
    for (tool, arg) in [(&t.ffmpeg, "-version"), (&t.ffprobe, "-version")] {
        if !runs(tool, arg) {
            return Err(format!("{tool} is missing: the recording cannot be cut"));
        }
    }
    if !Path::new(&rec.file).is_file() {
        return Err(format!("the recording {} is gone", rec.file));
    }
    let span = footage::span(rec, &c.window);
    let header = header_for(c.evidence.as_ref());
    for f in ["cut-16x9.mp4", "cut-9x16.mp4", "poster.png", "cut-16x9.ass"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let io = |e: ClipError| e.to_string();
    let land_ass = match &header {
        Some(h) => {
            let a = footage::landscape_ass(rec.width, rec.height, span.dur_ms, h);
            write_bytes(&dir.join("cut-16x9.ass"), a.as_bytes()).map_err(io)?;
            Some("cut-16x9.ass")
        }
        None => None,
    };
    footage::cut_landscape(&t.ffmpeg, dir, rec, span, land_ass)?;
    let rf = footage::reframe_for(c.segment);
    let vass = footage::vertical_ass(
        &c.captions,
        &rf,
        span.shift_ms,
        span.dur_ms,
        header.as_ref(),
        host,
    );
    write_bytes(&dir.join("captions-9x16.ass"), vass.as_bytes()).map_err(io)?;
    footage::cut_vertical(&t.ffmpeg, dir, rec, span, &rf, "captions-9x16.ass")?;
    let want = span.dur_ms as f64 / 1000.0;
    let mut durations = Vec::new();
    for (file, w, h) in [
        ("cut-16x9.mp4", rec.width, rec.height),
        ("cut-9x16.mp4", footage::V_WIDTH, footage::V_HEIGHT),
    ] {
        let (pw, ph, codec, dur) = ffprobe(&t.ffprobe, &dir.join(file))?;
        if (pw, ph) != (w, h) || codec != "h264" {
            return Err(format!("{file} is {pw}x{ph} {codec}, not {w}x{h} h264"));
        }
        if (dur - want).abs() > 0.5 {
            return Err(format!("{file} runs {dur:.2} s, not {want:.2} s"));
        }
        durations.push(dur);
    }
    // A still of the vertical cut at its middle, for lists and the poster slot.
    let _ = quiet(&mut Command::new(&t.ffmpeg))
        .current_dir(dir)
        .args(["-y", "-v", "error", "-ss"])
        .arg(format!("{:.3}", want / 2.0))
        .args(["-i", "cut-9x16.mp4", "-frames:v", "1", "poster.png"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let mut files = vec![
        file_entry(dir, "cut-9x16.mp4", "cut_9x16"),
        file_entry(dir, "cut-16x9.mp4", "cut_16x9"),
    ];
    if dir.join("poster.png").is_file() {
        files.push(file_entry(dir, "poster.png", "poster"));
    }
    files.push(file_entry(dir, "captions-9x16.ass", "captions_ass"));
    let overlay = ClipOverlay {
        captions: if c.window.voice_timed {
            CAPTION_SOURCE_VOICE.to_string()
        } else {
            format!("{CAPTION_SOURCE}; the 16:9 cut shows the stage's own captions")
        },
        ..overlay_for(header.as_ref())
    };
    Ok((
        ClipRender {
            kind: "video".into(),
            footage: Some(ClipFootage::Recording),
            width: footage::V_WIDTH,
            height: footage::V_HEIGHT,
            duration_s: (durations[1] * 100.0).round() / 100.0,
            files,
            overlay,
            fallback_reason: None,
            rendered_ms: now_ms(),
        },
        span,
    ))
}

/// Re-render a stored draft: always the poster set, plus the cuts from the recording when the
/// run has one, else the stage's re-drawn video when the tools allow.
pub fn render(dir: &Path, c: &ClipCandidate, t: Option<&RenderTools>) -> Result<ClipRender> {
    let text = std::fs::read_to_string(dir.join("clip.json"))
        .map_err(|e| ClipError::Io(format!("{}: {e}", dir.join("clip.json").display())))?;
    let input: ClipInput =
        serde_json::from_str(&text).map_err(|e| ClipError::Io(format!("clip.json: {e}")))?;
    let Some(tools) = t else {
        return write_draft(dir, c, &input, "no recording tools configured");
    };
    if let Some(rec) = &c.recording {
        let poster = write_draft(dir, c, &input, "")?;
        return match cut_from_recording(dir, c, tools, rec, &input.host) {
            Ok((mut r, span)) => {
                let keep = ["captions_srt", "captions_ass", "stage_input"];
                r.files.extend(
                    poster.files.into_iter().filter(|f| {
                        keep.contains(&f.kind.as_str()) && f.path != "captions-9x16.ass"
                    }),
                );
                write_manifest_with(dir, c, &r, Some((rec, span)))?;
                Ok(r)
            }
            Err(reason) => {
                for f in ["cut-16x9.mp4", "cut-9x16.mp4"] {
                    let _ = std::fs::remove_file(dir.join(f));
                }
                write_draft(
                    dir,
                    c,
                    &input,
                    &format!("cutting the recording failed: {reason}"),
                )
            }
        };
    }
    let probe = probe_tools(tools);
    match render_video(dir, tools, &probe, input.duration_ms) {
        Ok((dur, codec)) => {
            let mut r = write_draft(dir, c, &input, "")?;
            r.kind = "video".into();
            r.footage = Some(ClipFootage::Redrawn);
            r.duration_s = (dur * 100.0).round() / 100.0;
            r.fallback_reason = None;
            r.files.insert(0, file_entry(dir, "draft.mp4", "video"));
            if dir.join("poster.png").is_file() {
                r.files.insert(1, file_entry(dir, "poster.png", "poster"));
            }
            r.overlay.captions =
                format!("{CAPTION_SOURCE}; burnt into the {codec} video by the stage");
            write_manifest(dir, c, &r)?;
            Ok(r)
        }
        Err(reason) => {
            let _ = std::fs::remove_file(dir.join("draft.mp4"));
            write_draft(dir, c, &input, &reason)
        }
    }
}
