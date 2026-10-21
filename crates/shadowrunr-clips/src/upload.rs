//! The upload router: one adapter per platform behind [`UploadAdapter`], **dry run only**.
//! No credentials exist and this crate has no HTTP client; an adapter builds the exact
//! request it would send and the router writes it to `<data>/clips/<id>/requests/` instead.
//! Publishing refuses any candidate the owner has not moved to `approved_for_upload`, and a
//! dry run never moves a candidate to `published`.
//!
//! - YouTube: `videos.insert` (resumable upload) with `status.containsSyntheticMedia = true`,
//!   private until the owner flips it; the digest's 100/day bucket.
//! - TikTok: Content Posting API inbox draft (`/v2/post/publish/inbox/video/init/`), which the
//!   owner finishes in the app (Direct Post needs the platform's audit first).
//! - Twitch: Helix Create Clip, which clips the live broadcast's recent seconds and cannot take
//!   a rendered file: the dry run records it as not applicable to a draft.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use shadowrunr_types::clips::{
    ClipAct, ClipCandidate, ClipDryRun, ClipPlatform, ClipPublishReport,
};

use crate::render::{grade_name, DISCLOSURE};
use crate::store::{refuse_blocked, write_json, ClipStore};
use crate::{now_ms, Result};

/// Stands where a credential would go. Nothing is ever read from the environment.
pub const NO_CREDENTIAL: &str = "<absent: no credentials are configured; dry run>";

/// The exact request an adapter would send.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DryRequest {
    pub platform: ClipPlatform,
    pub dry_run: bool,
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: serde_json::Value,
    /// The file the upload step would stream, relative to the candidate's directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_file: Option<String>,
    pub applicable: bool,
    pub notes: Vec<String>,
}

pub trait UploadAdapter {
    fn platform(&self) -> ClipPlatform;
    /// Uploads allowed per UTC day, when the platform has a cap.
    fn daily_budget(&self) -> Option<u32>;
    fn request(&self, c: &ClipCandidate) -> DryRequest;
}

fn video(c: &ClipCandidate) -> Option<(String, u64)> {
    c.render.as_ref().and_then(|r| {
        r.files
            .iter()
            .find(|f| f.kind == "video")
            .map(|f| (f.path.clone(), f.bytes))
    })
}

fn description(c: &ClipCandidate) -> String {
    let mut d = format!(
        "{DISCLOSURE}. Arch-Lich Malakor is an original AI character; this clip is cut from a {} stream.",
        if c.source.tournament_id.is_some() { "simulated" } else { "recorded" }
    );
    if let Some(e) = &c.evidence {
        d.push_str(&format!(
            "\nALLEGED · grade {} · {}{}",
            grade_name(e.worst_grade),
            e.case_title,
            if e.fictional {
                " (fictional demo case)"
            } else {
                ""
            }
        ));
        for s in &e.citation {
            d.push_str(&format!("\nSource: {s}"));
        }
    }
    d
}

fn base_headers() -> BTreeMap<String, String> {
    let mut h = BTreeMap::new();
    h.insert("Authorization".into(), format!("Bearer {NO_CREDENTIAL}"));
    h
}

pub struct YouTube;

impl UploadAdapter for YouTube {
    fn platform(&self) -> ClipPlatform {
        ClipPlatform::Youtube
    }
    fn daily_budget(&self) -> Option<u32> {
        Some(100)
    }
    fn request(&self, c: &ClipCandidate) -> DryRequest {
        let file = video(c);
        let mut headers = base_headers();
        headers.insert(
            "Content-Type".into(),
            "application/json; charset=UTF-8".into(),
        );
        headers.insert("X-Upload-Content-Type".into(), "video/mp4".into());
        headers.insert(
            "X-Upload-Content-Length".into(),
            file.as_ref().map_or("0".into(), |(_, b)| b.to_string()),
        );
        let title: String = format!("{} #Shorts", c.title).chars().take(100).collect();
        let mut notes = vec![
            "resumable upload: this initiating request returns the upload URL the file is PUT to"
                .into(),
            "privacyStatus private: the owner publishes from YouTube Studio after a last look"
                .into(),
            "the digest's 100/day bucket is the cap here; the API's own unit quota may bind first"
                .into(),
        ];
        if file.is_none() {
            notes.push("no video rendered: YouTube needs the MP4 (run `clips render`)".into());
        }
        DryRequest {
            platform: ClipPlatform::Youtube,
            dry_run: true,
            method: "POST".into(),
            url: "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status".into(),
            headers,
            body: serde_json::json!({
                "snippet": {
                    "title": title,
                    "description": description(c),
                    "tags": ["Shadowrunr", "Malakor", "AI character"],
                    "categoryId": "24"
                },
                "status": {
                    "privacyStatus": "private",
                    "selfDeclaredMadeForKids": false,
                    "containsSyntheticMedia": true
                }
            }),
            upload_file: file.map(|(p, _)| p),
            applicable: video(c).is_some(),
            notes,
        }
    }
}

pub struct TikTok;

impl UploadAdapter for TikTok {
    fn platform(&self) -> ClipPlatform {
        ClipPlatform::Tiktok
    }
    fn daily_budget(&self) -> Option<u32> {
        None
    }
    fn request(&self, c: &ClipCandidate) -> DryRequest {
        let file = video(c);
        let size = file.as_ref().map_or(0, |(_, b)| *b);
        let mut headers = base_headers();
        headers.insert(
            "Content-Type".into(),
            "application/json; charset=UTF-8".into(),
        );
        let mut notes = vec![
            "inbox draft: the video lands in the creator's TikTok inbox and the owner finishes the post in the app, where the AI-generated content label is set".into(),
            "Direct Post (at most 15/day per the digest) needs the platform's API audit first".into(),
            format!("caption to paste: {}", description(c).replace('\n', " / ")),
        ];
        if file.is_none() {
            notes.push(
                "no video rendered: an inbox draft needs the MP4 (run `clips render`)".into(),
            );
        }
        DryRequest {
            platform: ClipPlatform::Tiktok,
            dry_run: true,
            method: "POST".into(),
            url: "https://open.tiktokapis.com/v2/post/publish/inbox/video/init/".into(),
            headers,
            body: serde_json::json!({
                "source_info": {
                    "source": "FILE_UPLOAD",
                    "video_size": size,
                    "chunk_size": size,
                    "total_chunk_count": 1
                }
            }),
            upload_file: file.map(|(p, _)| p),
            applicable: video(c).is_some(),
            notes,
        }
    }
}

pub struct Twitch;

impl UploadAdapter for Twitch {
    fn platform(&self) -> ClipPlatform {
        ClipPlatform::Twitch
    }
    fn daily_budget(&self) -> Option<u32> {
        None
    }
    fn request(&self, c: &ClipCandidate) -> DryRequest {
        let mut headers = base_headers();
        headers.insert("Client-Id".into(), NO_CREDENTIAL.into());
        DryRequest {
            platform: ClipPlatform::Twitch,
            dry_run: true,
            method: "POST".into(),
            url: "https://api.twitch.tv/helix/clips?broadcaster_id=<broadcaster id: none configured>&has_delay=false".into(),
            headers,
            body: serde_json::Value::Null,
            upload_file: None,
            applicable: false,
            notes: vec![
                "Create Clip captures the live broadcast's most recent seconds; it cannot upload a rendered file".into(),
                format!(
                    "this candidate comes from {} with no live Twitch broadcast, so the call does not apply",
                    if c.source.tournament_id.is_some() { "a simulated run" } else { "a stored transcript" }
                ),
            ],
        }
    }
}

pub fn adapter_for(p: ClipPlatform) -> Box<dyn UploadAdapter> {
    match p {
        ClipPlatform::Youtube => Box::new(YouTube),
        ClipPlatform::Tiktok => Box::new(TikTok),
        ClipPlatform::Twitch => Box::new(Twitch),
    }
}

pub const ALL_PLATFORMS: [ClipPlatform; 3] = [
    ClipPlatform::Youtube,
    ClipPlatform::Tiktok,
    ClipPlatform::Twitch,
];

fn platform_name(p: ClipPlatform) -> &'static str {
    match p {
        ClipPlatform::Youtube => "youtube",
        ClipPlatform::Tiktok => "tiktok",
        ClipPlatform::Twitch => "twitch",
    }
}

/// Real uploads per platform per UTC day (`<data>/clips/upload-budget.json`). Nothing in this
/// build uploads, so it only ever reads zero; the file is the record a real adapter keeps.
#[derive(Debug, Default, Serialize, Deserialize)]
struct BudgetFile {
    #[serde(default)]
    days: BTreeMap<String, BTreeMap<String, u32>>,
}

fn utc_day(ms: i64) -> String {
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Dry-run publish: refused unless `approved_for_upload`; writes one request file per
/// platform and records each on the candidate.
pub fn publish_dry_run(
    store: &ClipStore,
    id: &str,
    platforms: Option<&[ClipPlatform]>,
) -> Result<ClipPublishReport> {
    let _lock = store.lock()?;
    let mut c = store.load(id)?;
    refuse_blocked(&c, ClipAct::Publish)?;
    let now = now_ms();
    let day = utc_day(now);
    let budget: BudgetFile = std::fs::read_to_string(store.root().join("upload-budget.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let dir = store.dir(id);
    let mut requests = Vec::new();
    for p in platforms.unwrap_or(&ALL_PLATFORMS) {
        let a = adapter_for(*p);
        let req = a.request(&c);
        let rel = format!("requests/{}.json", platform_name(*p));
        write_json(&dir.join(&rel), &req)?;
        let used = budget
            .days
            .get(&day)
            .and_then(|d| d.get(platform_name(*p)))
            .copied()
            .unwrap_or(0);
        let rec = ClipDryRun {
            platform: *p,
            at_ms: now,
            file: rel,
            applicable: req.applicable,
            budget_left: a.daily_budget().map(|b| b.saturating_sub(used)),
            note: req.notes.first().cloned().unwrap_or_default(),
        };
        c.dry_runs.push(rec.clone());
        requests.push(rec);
    }
    store.save(&c)?;
    Ok(ClipPublishReport {
        id: id.to_string(),
        dry_run: true,
        requests,
    })
}

#[cfg(test)]
mod tests {
    use super::utc_day;

    #[test]
    fn utc_days() {
        assert_eq!(utc_day(0), "1970-01-01");
        assert_eq!(utc_day(1_760_832_000_000), "2025-10-19");
        assert_eq!(utc_day(951_782_400_000), "2000-02-29");
    }
}
