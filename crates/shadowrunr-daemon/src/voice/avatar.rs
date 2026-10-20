//! `GET /avatar/{file}`: the face rig from `<data>/avatar/rig/`. The name must be one safe
//! path segment (`[A-Za-z0-9._-]+`, no `..`) with a known extension (`json`, `png`, `webp`);
//! anything else, and any file that is not there, is 404, so the stage draws its placeholder.
//! `cache-control: no-cache`: a re-split rig shows on the next load.

use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use super::rig_dir;
use crate::server::AppState;

/// The content type for a safe rig file name; `None` refuses it.
pub fn rig_file_type(name: &str) -> Option<&'static str> {
    let safe = !name.is_empty()
        && name.len() <= 128
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !safe {
        return None;
    }
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "json" => Some("application/json"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub async fn avatar_handler(
    State(state): State<Arc<AppState>>,
    UrlPath(file): UrlPath<String>,
) -> Response {
    let Some(ct) = rig_file_type(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = rig_dir(&state.cfg.paths.data).join(&file);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, ct),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_safe_segment_with_a_rig_extension_is_served() {
        assert_eq!(rig_file_type("layers.json"), Some("application/json"));
        assert_eq!(rig_file_type("mouth_aa-2.PNG"), Some("image/png"));
        assert_eq!(rig_file_type("eyes.webp"), Some("image/webp"));
        for bad in [
            "",
            "..",
            "../secrets.json",
            "..\\x.png",
            "a..json",
            "rig/layers.json",
            "C:layers.json",
            "layers",
            "layers.js",
            "notes.txt",
            "%2e%2e.json",
            "lay ers.json",
        ] {
            assert_eq!(rig_file_type(bad), None, "{bad}");
        }
    }
}
