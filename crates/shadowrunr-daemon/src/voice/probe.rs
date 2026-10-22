//! What the voice needs on this machine, item by item: Kokoro's files and the engine binary
//! (from `engine_dir`), the python and the sidecar package, and, for mouth shapes, the durations
//! model and numpy/onnxruntime, and a configured voices file (missing, or not the model's size,
//! is the sidecar's `degraded` before it starts). `shadowrunr voice check` prints the table; the service runs the
//! file half before it spawns anything, so missing Kokoro files are `degraded` with their name,
//! not a crash loop.
//!
//! The binary is not needed by itself: the sidecar's warm engine keeps Kokoro loaded in its own
//! workers and never runs it. Whether the sidecar can speak without it is the sidecar's call,
//! made in its `hello` (whose `reason` names the engine in use); [`hello_state`] reads it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::renderer::RendererState;
use super::VoiceConfig;

/// The engine binary under `<engine_dir>/bin`.
pub const ENGINE_BIN: &str = if cfg!(windows) {
    "sherpa-onnx-offline-tts.exe"
} else {
    "sherpa-onnx-offline-tts"
};

/// What a `hello` reason names when the warm engine runs.
pub const WARM_ENGINE: &str = "engine: warm";

/// Kokoro's files under `<engine_dir>/kokoro`, all needed.
pub const KOKORO_FILES: [&str; 4] = ["model.onnx", "voices.bin", "tokens.txt", "espeak-ng-data"];

/// One speaker's style rows in a voices file: 510 rows of 256 float32 (the sidecar's
/// `engine.SPEAKER_BYTES`).
pub const SPEAKER_BYTES: u64 = 510 * 256 * 4;
/// kokoro-multi-lang-v1_0's speakers: the voices file's size when the stock one is not there.
pub const SPEAKERS: u64 = 53;

/// How long a python probe may take.
const PY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Ok,
    Missing,
}

/// One row of `voice check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub item: String,
    pub state: ItemState,
    /// Needed to speak at all; the rest only add mouth shapes.
    pub needed: bool,
    pub detail: String,
}

impl Item {
    fn new(item: &str, ok: bool, needed: bool, detail: impl Into<String>) -> Self {
        Self {
            item: item.into(),
            state: if ok {
                ItemState::Ok
            } else {
                ItemState::Missing
            },
            needed,
            detail: detail.into(),
        }
    }

    pub fn ok(&self) -> bool {
        self.state == ItemState::Ok
    }
}

/// The engine binary and Kokoro's files. The binary is optional (the warm engine speaks
/// without it); Kokoro's files are needed either way.
pub fn engine_items(engine_dir: Option<&str>) -> Vec<Item> {
    let Some(dir) = engine_dir else {
        return vec![Item::new(
            "engine",
            false,
            true,
            "engine_dir is not set in [voice]",
        )];
    };
    let dir = PathBuf::from(dir);
    let bin = Path::new("bin").join(ENGINE_BIN);
    let mut engine = file_item("engine", &dir, &bin, false);
    if !engine.ok() {
        engine.detail.push_str(" (only the warm engine can speak)");
    }
    let mut items = vec![engine];
    for f in KOKORO_FILES {
        items.push(file_item(
            &format!("kokoro {f}"),
            &dir,
            &Path::new("kokoro").join(f),
            true,
        ));
    }
    items
}

fn file_item(item: &str, root: &Path, rel: &Path, needed: bool) -> Item {
    let ok = root.join(rel).exists();
    let rel = rel.to_string_lossy();
    Item::new(
        item,
        ok,
        needed,
        if ok {
            format!("{rel} in engine_dir")
        } else {
            format!("{rel} is not in engine_dir")
        },
    )
}

/// The configured voices file (`voices_file`, from `[voice]` or the character's package), the
/// sidecar's own test before it speaks: it must exist, be the model's `kokoro/voices.bin` size
/// exactly (sherpa-onnx refuses any other) and hold `sid`. Named, never by its path. `None`
/// when no voices file is set (the model's own is one of the Kokoro rows).
pub fn voices_item(cfg: &VoiceConfig) -> Option<Item> {
    let file = cfg.voices_file.as_deref()?;
    let Some(path) = cfg.voices_path() else {
        return Some(Item::new(
            "voices file",
            false,
            true,
            format!("{file}: no voices_dir and no home directory to find it in"),
        ));
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());
    let Some(size) = std::fs::metadata(&path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
    else {
        return Some(Item::new(
            "voices file",
            false,
            true,
            format!("the voices file {name} is missing"),
        ));
    };
    let stock = cfg
        .engine_dir
        .as_deref()
        .and_then(|d| std::fs::metadata(Path::new(d).join("kokoro").join("voices.bin")).ok())
        .map(|m| m.len())
        .filter(|&n| n > 0);
    let want = stock.unwrap_or(SPEAKERS * SPEAKER_BYTES);
    if size != want {
        return Some(Item::new(
            "voices file",
            false,
            true,
            format!("the voices file {name} is {size} bytes; the model's voices.bin is {want}"),
        ));
    }
    let speakers = size / SPEAKER_BYTES;
    if u64::from(cfg.sid) >= speakers {
        return Some(Item::new(
            "voices file",
            false,
            true,
            format!(
                "sid {} is not one of the {speakers} speakers in the voices file {name}",
                cfg.sid
            ),
        ));
    }
    Some(Item::new(
        "voices file",
        true,
        true,
        format!("{name}, sid {} of {speakers} speakers", cfg.sid),
    ))
}

/// The first needed engine file that is missing, as a `degraded` reason.
pub fn engine_missing(cfg: &VoiceConfig) -> Option<String> {
    engine_items(cfg.engine_dir.as_deref())
        .into_iter()
        .find(|i| i.needed && !i.ok())
        .map(|i| format!("voice engine missing: {}", i.detail))
}

/// Whether `<engine_dir>/bin` lacks the engine binary.
pub fn engine_bin_missing(cfg: &VoiceConfig) -> bool {
    cfg.engine_dir
        .as_deref()
        .is_none_or(|d| !Path::new(d).join("bin").join(ENGINE_BIN).exists())
}

/// The renderer's state on the sidecar's `hello`. With the binary there, `ready`. Without it
/// only the warm engine can speak, so the hello must name it; any other hello is `degraded`,
/// saying that neither engine can run and what the sidecar said.
pub fn hello_state(cfg: &VoiceConfig, visemes: bool, reason: Option<String>) -> RendererState {
    let warm = reason.as_deref().is_some_and(|r| r.contains(WARM_ENGINE));
    if warm || !engine_bin_missing(cfg) {
        return RendererState::Ready { visemes, reason };
    }
    let bin = Path::new("bin").join(ENGINE_BIN);
    RendererState::Degraded(format!(
        "voice engine missing: {} is not in engine_dir and the warm engine is not running ({})",
        bin.to_string_lossy(),
        reason.as_deref().unwrap_or("the sidecar gave no reason")
    ))
}

/// The durations model (optional).
pub fn durations_item(cfg: &VoiceConfig) -> Item {
    match &cfg.durations_model {
        None => Item::new(
            "durations model",
            false,
            false,
            "durations_model is not set (no mouth shapes)",
        ),
        Some(p) => {
            let ok = Path::new(p).is_file();
            let name = Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Item::new(
                "durations model",
                ok,
                false,
                if ok {
                    name
                } else {
                    format!("{name} not found (no mouth shapes)")
                },
            )
        }
    }
}

/// Run `python -c <code>` in the sidecar directory: Ok(stdout) or Err(why).
async fn py(cfg: &VoiceConfig, code: &str) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(&cfg.python);
    cmd.arg("-c").arg(code).kill_on_drop(true);
    if Path::new(&cfg.sidecar).is_dir() {
        cmd.current_dir(&cfg.sidecar);
    }
    #[cfg(windows)]
    cmd.creation_flags(super::sidecar::CREATE_NO_WINDOW);
    let out = match tokio::time::timeout(PY_TIMEOUT, cmd.output()).await {
        Err(_) => return Err(format!("no answer in {} s", PY_TIMEOUT.as_secs())),
        Ok(Err(e)) => return Err(format!("cannot run {}: {e}", cfg.python)),
        Ok(Ok(o)) => o,
    };
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    Err(err
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed")
        .trim()
        .to_string())
}

/// The python, the sidecar import, and the mouth-shape packages.
pub async fn python_items(cfg: &VoiceConfig) -> Vec<Item> {
    let version = py(cfg, "import sys; print(sys.version.split()[0])").await;
    let Ok(version) = version else {
        let why = version.unwrap_err();
        return vec![
            Item::new("python", false, true, why),
            Item::new("sidecar", false, true, "needs the python"),
        ];
    };
    let mut items = vec![Item::new("python", true, true, format!("Python {version}"))];
    items.push(if !Path::new(&cfg.sidecar).is_dir() {
        Item::new(
            "sidecar",
            false,
            true,
            "the sidecar directory does not exist",
        )
    } else {
        match py(cfg, "import shadowrunr_voice").await {
            Ok(_) => Item::new("sidecar", true, true, "shadowrunr_voice imports"),
            Err(e) => Item::new("sidecar", false, true, e),
        }
    });
    items.push(match py(cfg, "import numpy, onnxruntime").await {
        Ok(_) => Item::new("mouth-shape packages", true, false, "numpy, onnxruntime"),
        Err(e) => Item::new(
            "mouth-shape packages",
            false,
            false,
            format!("{e} (no mouth shapes)"),
        ),
    });
    items
}

/// Every item, in table order.
pub async fn check(cfg: &VoiceConfig) -> Vec<Item> {
    let mut items = engine_items(cfg.engine_dir.as_deref());
    items.extend(voices_item(cfg));
    items.extend(python_items(cfg).await);
    items.push(durations_item(cfg));
    items
}

/// Whether everything needed to speak is present.
pub fn all_needed(items: &[Item]) -> bool {
    items.iter().all(|i| i.ok() || !i.needed)
}

/// The aligned table `voice check` prints.
pub fn table(items: &[Item]) -> String {
    let w = items.iter().map(|i| i.item.len()).max().unwrap_or(4).max(4);
    let mut out = format!("{:<w$}  {:<8} detail\n", "item", "state");
    for i in items {
        let state = match (i.state, i.needed) {
            (ItemState::Ok, _) => "ok",
            (ItemState::Missing, true) => "MISSING",
            (ItemState::Missing, false) => "missing",
        };
        out.push_str(&format!("{:<w$}  {state:<8} {}\n", i.item, i.detail));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_files_are_probed_relative_to_engine_dir() {
        let d = tempfile::tempdir().unwrap();
        let none = engine_items(None);
        assert_eq!(none.len(), 1);
        assert!(!all_needed(&none));

        let dir = d.path().to_string_lossy().into_owned();
        let items = engine_items(Some(&dir));
        assert_eq!(items.len(), 5);
        assert!(items.iter().all(|i| !i.ok()));
        assert!(!items[0].needed && items[1..].iter().all(|i| i.needed));
        let cfg = VoiceConfig {
            engine_dir: Some(dir.clone()),
            ..VoiceConfig::default()
        };
        assert!(engine_missing(&cfg).unwrap().contains("model.onnx"));

        // Kokoro's files alone are enough to spawn: the warm engine needs no binary.
        std::fs::create_dir_all(d.path().join("kokoro/espeak-ng-data")).unwrap();
        for f in ["model.onnx", "voices.bin", "tokens.txt"] {
            std::fs::write(d.path().join("kokoro").join(f), b"").unwrap();
        }
        let items = engine_items(Some(&dir));
        assert!(all_needed(&items));
        assert!(
            items[0].detail.contains("only the warm engine"),
            "{}",
            items[0].detail
        );
        assert_eq!(engine_missing(&cfg), None);
        assert!(engine_bin_missing(&cfg));

        std::fs::create_dir_all(d.path().join("bin")).unwrap();
        std::fs::write(d.path().join("bin").join(ENGINE_BIN), b"").unwrap();
        assert!(engine_items(Some(&dir)).iter().all(Item::ok));
        assert!(!engine_bin_missing(&cfg));
    }

    #[test]
    fn without_the_binary_only_a_warm_hello_is_ready() {
        let d = tempfile::tempdir().unwrap();
        let cfg = VoiceConfig {
            engine_dir: Some(d.path().to_string_lossy().into_owned()),
            ..VoiceConfig::default()
        };
        let warm = Some("no numpy; engine: warm (3 workers loaded in 2.1 s)".to_string());
        assert_eq!(
            hello_state(&cfg, false, warm.clone()),
            RendererState::Ready {
                visemes: false,
                reason: warm
            }
        );
        let dead = Some("the engine is not installed: bin/x is missing".to_string());
        match hello_state(&cfg, false, dead) {
            RendererState::Degraded(why) => {
                assert!(why.contains(ENGINE_BIN), "{why}");
                assert!(why.contains("warm engine is not running"), "{why}");
                assert!(why.contains("the engine is not installed"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            hello_state(&cfg, true, None),
            RendererState::Degraded(_)
        ));
        // With the binary, any hello is ready (the process engine speaks).
        std::fs::create_dir_all(d.path().join("bin")).unwrap();
        std::fs::write(d.path().join("bin").join(ENGINE_BIN), b"").unwrap();
        assert_eq!(
            hello_state(&cfg, true, Some("engine: process".into())),
            RendererState::Ready {
                visemes: true,
                reason: Some("engine: process".into())
            }
        );
    }

    #[test]
    fn a_missing_or_wrong_sized_voices_file_is_a_needed_row() {
        let d = tempfile::tempdir().unwrap();
        let engine = d.path().join("engine");
        std::fs::create_dir_all(engine.join("kokoro")).unwrap();
        let stock = vec![0u8; (SPEAKER_BYTES * 3) as usize];
        std::fs::write(engine.join("kokoro").join("voices.bin"), &stock).unwrap();
        let voices = d.path().join("voices");
        std::fs::create_dir_all(&voices).unwrap();
        let mut cfg = VoiceConfig {
            engine_dir: Some(engine.to_string_lossy().into_owned()),
            voices_dir: Some(voices.to_string_lossy().into_owned()),
            sid: 2,
            ..VoiceConfig::default()
        };
        // None configured: no row (the model's own file is a Kokoro row).
        assert_eq!(voices_item(&cfg), None);

        cfg.voices_file = Some("lich-voices.bin".into());
        let missing = voices_item(&cfg).unwrap();
        assert!(missing.needed && !missing.ok());
        assert_eq!(missing.detail, "the voices file lich-voices.bin is missing");
        assert!(!all_needed(&[missing]));

        let file = voices.join("lich-voices.bin");
        std::fs::write(&file, vec![0u8; (SPEAKER_BYTES * 2) as usize]).unwrap();
        let short = voices_item(&cfg).unwrap();
        assert!(!short.ok());
        assert!(
            short
                .detail
                .contains(&format!("{} bytes", SPEAKER_BYTES * 2))
                && short.detail.contains(&format!("is {}", SPEAKER_BYTES * 3)),
            "{}",
            short.detail
        );
        assert!(
            !short.detail.contains(&*voices.to_string_lossy()),
            "never the path"
        );

        std::fs::write(&file, &stock).unwrap();
        let ok = voices_item(&cfg).unwrap();
        assert!(ok.ok(), "{}", ok.detail);
        assert_eq!(ok.detail, "lich-voices.bin, sid 2 of 3 speakers");
        cfg.sid = 3;
        assert!(voices_item(&cfg)
            .unwrap()
            .detail
            .contains("sid 3 is not one of the 3"));

        // Without the stock file the model's size is 53 speakers.
        std::fs::remove_file(engine.join("kokoro").join("voices.bin")).unwrap();
        cfg.sid = 2;
        let unsized_ = voices_item(&cfg).unwrap();
        assert!(
            unsized_
                .detail
                .contains(&format!("is {}", SPEAKERS * SPEAKER_BYTES)),
            "{}",
            unsized_.detail
        );
        // An absolute voices_file is read as is.
        cfg.voices_file = Some(file.to_string_lossy().into_owned());
        cfg.voices_dir = None;
        assert!(voices_item(&cfg)
            .unwrap()
            .detail
            .starts_with("the voices file lich-voices.bin is"));
    }

    #[test]
    fn optional_items_do_not_fail_the_check() {
        let cfg = VoiceConfig::default();
        let d = durations_item(&cfg);
        assert!(!d.ok() && !d.needed);
        let items = vec![
            Item::new("engine", true, true, "x"),
            Item::new("durations model", false, false, "not set"),
        ];
        assert!(all_needed(&items));
        let t = table(&items);
        assert!(t.contains("engine           ok       x"), "{t}");
        assert!(t.contains("durations model  missing  not set"), "{t}");
        assert!(!all_needed(&[Item::new("python", false, true, "gone")]));
        assert!(table(&[Item::new("python", false, true, "gone")]).contains("MISSING"));
    }
}
