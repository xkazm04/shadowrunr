//! `GET /media/clips/{id}/{file}` and `GET /media/runs/sandbox/{session}/{file}`: the cuts and
//! the show's recording, for the console's `<video>` player.
//!
//! - Only fixed file names are served: a clip's `cut-16x9.mp4`, `cut-9x16.mp4`, `draft.mp4`
//!   and `poster.png`; a sandbox session's `recording.mp4` (its timing comes over
//!   `clips.recording`, through the contract's door).
//!   The clip id must be `clip-<16 hex>` and the session id a sandbox session id, so no path
//!   reaches outside `<data>/clips/<id>/` or `<runs>/sandbox/<session>/`. Anything else is 404.
//! - Origins are restricted as on every other route (`server::origin_ok`): a request carrying
//!   any other `Origin` is 403. A `<video>` element's own requests carry none.
//! - `Range: bytes=a-b` (one range; `a-` and `-n` too) answers 206 with `Content-Range`, which
//!   is how the player seeks; an unsatisfiable range is 416. The body streams from disk in
//!   chunks, so a recording of hundreds of megabytes is never read whole.

use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::server::{origin_ok, AppState};

const CHUNK: usize = 256 * 1024;

/// The content type of a clip file the console may load; `None` refuses the name.
pub fn clip_file_type(name: &str) -> Option<&'static str> {
    match name {
        "cut-16x9.mp4" | "cut-9x16.mp4" | "draft.mp4" => Some("video/mp4"),
        "poster.png" => Some("image/png"),
        _ => None,
    }
}

/// The content type of a sandbox session file the console may load; `None` refuses the name.
pub fn run_file_type(name: &str) -> Option<&'static str> {
    match name {
        "recording.mp4" => Some("video/mp4"),
        _ => None,
    }
}

/// One byte range of a `len`-byte file: `Ok(None)` for no (or an ignorable) header,
/// `Ok(Some((start, end)))` inclusive, `Err(())` when it cannot be satisfied.
#[allow(clippy::result_unit_err)]
pub fn parse_range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(h) = header else { return Ok(None) };
    let Some(spec) = h.trim().strip_prefix("bytes=") else {
        return Ok(None);
    };
    // Several ranges would need a multipart reply: serve the whole file instead.
    if spec.contains(',') {
        return Ok(None);
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (a, b) = (a.trim(), b.trim());
    let num = |s: &str| s.parse::<u64>().map_err(|_| ());
    let (start, end) = match (a.is_empty(), b.is_empty()) {
        (true, true) => return Err(()),
        // The last n bytes.
        (true, false) => {
            let n = num(b)?;
            if n == 0 || len == 0 {
                return Err(());
            }
            (len.saturating_sub(n), len - 1)
        }
        (false, true) => (num(a)?, len.saturating_sub(1)),
        (false, false) => (num(a)?, num(b)?.min(len.saturating_sub(1))),
    };
    if start >= len || start > end {
        return Err(());
    }
    Ok(Some((start, end)))
}

pub async fn clip_media_handler(
    State(state): State<Arc<AppState>>,
    UrlPath((id, file)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(ct) = clip_file_type(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if shadowrunr_clips::store::check_id(&id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve(state.cfg.clip_store().dir(&id).join(&file), ct, &headers).await
}

pub async fn run_media_handler(
    State(state): State<Arc<AppState>>,
    UrlPath((session, file)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !origin_ok(&headers, &state.cfg.daemon.extra_origins) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(ct) = run_file_type(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if crate::review::check_session_id(&session).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    serve(
        state.cfg.paths.sandbox_runs().join(&session).join(&file),
        ct,
        &headers,
    )
    .await
}

async fn serve(path: PathBuf, ct: &'static str, headers: &HeaderMap) -> Response {
    let Ok(mut f) = tokio::fs::File::open(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let len = match f.metadata().await {
        Ok(m) if m.is_file() => m.len(),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (status, start, end) = match parse_range(range, len) {
        Ok(None) => (StatusCode::OK, 0, len.saturating_sub(1)),
        Ok(Some((a, b))) => (StatusCode::PARTIAL_CONTENT, a, b),
        Err(()) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{len}"))],
            )
                .into_response()
        }
    };
    let count = if len == 0 { 0 } else { end - start + 1 };
    if start > 0 && f.seek(SeekFrom::Start(start)).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let body = futures::stream::unfold((f, count), |(mut f, left)| async move {
        if left == 0 {
            return None;
        }
        let mut buf = vec![0u8; (left as usize).min(CHUNK)];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((
                    Ok::<_, std::io::Error>(Bytes::from(buf)),
                    (f, left - n as u64),
                ))
            }
            Err(e) => Some((Err(e), (f, 0))),
        }
    });
    let mut r = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, ct)
        .header(header::CONTENT_LENGTH, count)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-cache");
    if status == StatusCode::PARTIAL_CONTENT {
        r = r.header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
    }
    r.body(Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_parse_as_the_player_sends_them() {
        assert_eq!(parse_range(None, 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=0-"), 100), Ok(Some((0, 99))));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Ok(Some((10, 19))));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Ok(Some((0, 99))));
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), Ok(None));
        assert_eq!(parse_range(Some("items=0-1"), 100), Ok(None));
        for bad in [
            "bytes=100-",
            "bytes=20-10",
            "bytes=-0",
            "bytes=-",
            "bytes=x-1",
        ] {
            assert_eq!(parse_range(Some(bad), 100), Err(()), "{bad}");
        }
        assert_eq!(parse_range(Some("bytes=0-"), 0), Err(()));
    }

    #[test]
    fn only_the_named_files_are_served() {
        assert_eq!(clip_file_type("cut-9x16.mp4"), Some("video/mp4"));
        assert_eq!(run_file_type("recording.mp4"), Some("video/mp4"));

        for bad in [
            "",
            "..",
            "../candidate.json",
            "candidate.json",
            "poster.svg",
            "CUT-9X16.MP4",
            "cut-9x16.mp4/..",
            "..\\recording.mp4",
        ] {
            assert_eq!(clip_file_type(bad), None, "{bad}");
            assert_eq!(run_file_type(bad), None, "{bad}");
        }
        for other in [
            "transcript.jsonl",
            "recording.json",
            "voice.jsonl",
            "session.json",
        ] {
            assert_eq!(run_file_type(other), None, "{other}");
        }
        assert_eq!(run_file_type("recording.mkv"), None);
    }
}
