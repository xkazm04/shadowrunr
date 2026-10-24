//! A foil's own voice: the SEG-03 guest's, or a SEG-01 foil's once it is summoned.
//!
//! - **Where it is set.** The host's package holds it, `characters/<host>/voice/foils/<foil>.toml`,
//!   and the operator's local file `<data>/characters/<host>/voice/foils/<foil>.toml` (beside the
//!   host's `voice.json`, never committed) wins over it field by field. Both are optional: a foil
//!   with neither is not voiced, and its lines are shown as captions only.
//! - **What it holds.** The Kokoro fields of the host's `[voice]`: `sid` (required once the two
//!   are merged), `voice` (the label; the foil id when absent), `voices_file` (absent or `""`: the
//!   model's own `voices.bin`, never the host's blend; `""` in the local file undoes the
//!   package's blend), `fx` (absent: none), and `workers` (the foil's
//!   sidecar's engine runs; [`DEFAULT_WORKERS`] when absent). Everything else (python, engine,
//!   durations model, timeouts) is the daemon's `[voice]`.
//! - **Kokoro only (v1).** A foil's voice is always a Kokoro sidecar of its own, whichever
//!   provider the host speaks with: [`resolve`] builds nothing else, so a host on ElevenLabs
//!   still has a Kokoro guest.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use shadowrunr_types::character::VoiceFx;
use shadowrunr_types::voice_choice::{KokoroVoice, VoiceChoice, VoiceProvider};

use super::service::Spoken;
use super::settings::{check_choice, kokoro_key, Spec};
use crate::config::Config;
use crate::runs;

/// A foil's sidecar renders this many sentences at once unless its file says otherwise: its
/// lines are short and it shares the machine with the host's sidecar.
pub const DEFAULT_WORKERS: usize = 1;

/// A foil voice file (the package's or the local one).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoilVoice {
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub sid: Option<u32>,
    #[serde(default)]
    pub voices_file: Option<String>,
    #[serde(default)]
    pub fx: Option<VoiceFx>,
    #[serde(default)]
    pub workers: Option<usize>,
}

impl FoilVoice {
    /// `self` with `over` on top, field by field.
    pub fn under(self, over: FoilVoice) -> FoilVoice {
        FoilVoice {
            voice: over.voice.or(self.voice),
            sid: over.sid.or(self.sid),
            voices_file: over.voices_file.or(self.voices_file),
            fx: over.fx.or(self.fx),
            workers: over.workers.or(self.workers),
        }
    }
}

fn file(dir: PathBuf, foil: &str) -> PathBuf {
    dir.join("voice").join("foils").join(format!("{foil}.toml"))
}

/// `characters/<host>/voice/foils/<foil>.toml`.
pub fn package_path(characters: &Path, host: &str, foil: &str) -> PathBuf {
    file(characters.join(host), foil)
}

/// `<data>/characters/<host>/voice/foils/<foil>.toml`.
pub fn override_path(data: &Path, host: &str, foil: &str) -> PathBuf {
    file(data.join("characters").join(host), foil)
}

/// A voice file, `None` when there is none.
pub fn read(path: &Path) -> Result<Option<FoilVoice>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// The merged voice of `foil` in `host`'s streams, `None` when neither file exists.
pub fn merged(cfg: &Config, host: &str, foil: &str) -> Result<Option<FoilVoice>, String> {
    runs::check_id("character", host).map_err(|e| e.message)?;
    runs::check_id("foil", foil).map_err(|e| e.message)?;
    let package = read(&package_path(&cfg.paths.characters, host, foil))?;
    let over = read(&override_path(&cfg.paths.data, host, foil))?;
    Ok(match (package, over) {
        (None, None) => None,
        (p, o) => Some(p.unwrap_or_default().under(o.unwrap_or_default())),
    })
}

/// The voice `foil` speaks with in `host`'s streams: `Ok(None)` when none is configured (its
/// lines are captions only), an error naming the file when one cannot be used.
pub fn resolve(cfg: &Config, host: &str, foil: &str) -> Result<Option<Spoken>, String> {
    let Some(m) = merged(cfg, host, foil)? else {
        return Ok(None);
    };
    let Some(sid) = m.sid else {
        return Err(format!(
            "{foil}'s voice sets no sid ({})",
            package_path(&cfg.paths.characters, host, foil).display()
        ));
    };
    // `""` is the model's own file: the local file's way to drop the package's blend.
    let voices_file = m.voices_file.filter(|f| !f.is_empty());
    // The same rules a host's stored choice is held to (plain names, no paths).
    check_choice(&VoiceChoice {
        provider: VoiceProvider::Kokoro,
        kokoro: Some(KokoroVoice {
            voice: m.voice.clone(),
            sid: Some(sid),
            voices_file: voices_file.clone(),
            fx: m.fx,
            speed: None,
        }),
        elevenlabs: None,
    })
    .map_err(|e| format!("{foil}'s voice: {}", e.message))?;
    let mut v = cfg.voice_daemon.clone();
    v.enabled = cfg.voice.enabled;
    v.voice = m.voice.unwrap_or_else(|| foil.to_string());
    v.sid = sid;
    v.voices_file = voices_file;
    v.fx = m.fx;
    v.render_workers = m.workers.unwrap_or(DEFAULT_WORKERS).max(1);
    Ok(Some(Spoken {
        provider: VoiceProvider::Kokoro,
        voice: v.voice.clone(),
        key: kokoro_key(&v),
        spec: Spec::Kokoro(Box::new(v)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2e::{copy_dir, repo, test_config};
    use shadowrunr_types::voice_choice::ElevenlabsVoice;

    fn setup(tmp: &Path) -> Config {
        let chars = tmp.join("characters");
        copy_dir(&repo().join("characters/malakor"), &chars.join("malakor"));
        let mut cfg = test_config(tmp, chars);
        cfg.voice.enabled = true;
        cfg
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn kokoro(s: &Spoken) -> &crate::voice::VoiceConfig {
        match &s.spec {
            Spec::Kokoro(v) => v,
            Spec::Elevenlabs(..) => panic!("a foil is never on ElevenLabs"),
        }
    }

    #[test]
    fn the_painter_has_a_committed_voice_on_its_own_blend() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = setup(tmp.path());
        let s = resolve(&cfg, "malakor", "the-painter").unwrap().unwrap();
        assert_eq!(s.provider, VoiceProvider::Kokoro);
        let v = kokoro(&s);
        assert_eq!(
            (v.voice.as_str(), v.sid, v.voices_file.as_deref()),
            ("the-painter-a", 52, Some("the-painter-voices.bin"))
        );
        assert_eq!(v.render_workers, DEFAULT_WORKERS);
        // Not the host's voice, and not the host's renderer.
        let host = crate::voice::settings::spoken_for(&cfg, "malakor");
        assert_ne!(s.key, host.key);
    }

    #[test]
    fn a_foil_without_a_file_is_unvoiced_and_a_bad_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = setup(tmp.path());
        assert!(resolve(&cfg, "malakor", "the-disruptor").unwrap().is_none());
        let pkg = package_path(&cfg.paths.characters, "malakor", "the-disruptor");
        write(&pkg, "voice = \"x\"\n");
        assert!(resolve(&cfg, "malakor", "the-disruptor")
            .unwrap_err()
            .contains("no sid"));
        write(&pkg, "sid = 1\npitch = 3\n");
        assert!(resolve(&cfg, "malakor", "the-disruptor").is_err());
        write(&pkg, "sid = 1\nvoices_file = \"../x.bin\"\n");
        assert!(resolve(&cfg, "malakor", "the-disruptor").is_err());
        assert!(resolve(&cfg, "malakor", "../etc").is_err());
    }

    #[test]
    fn the_local_file_wins_per_field_and_a_stock_voice_needs_no_blend() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = setup(tmp.path());
        // The stock fallback: bm_fable is sid 25 of kokoro-multi-lang-v1_0's own voices.bin.
        write(
            &override_path(&cfg.paths.data, "malakor", "the-painter"),
            "# local\nvoice = \"bm_fable\"\nsid = 25\nvoices_file = \"\"\nworkers = 2\n",
        );
        let v = resolve(&cfg, "malakor", "the-painter").unwrap().unwrap();
        let k = kokoro(&v);
        assert_eq!(
            (k.voice.as_str(), k.sid, k.render_workers),
            ("bm_fable", 25, 2)
        );
        assert_eq!(k.voices_file, None, "\"\" is the model's own voices.bin");
        assert_eq!(k.fx, None, "the host's fx is not the guest's");
        // Without the local file the package's blend speaks.
        std::fs::remove_file(override_path(&cfg.paths.data, "malakor", "the-painter")).unwrap();
        let v = resolve(&cfg, "malakor", "the-painter").unwrap().unwrap();
        assert_eq!(
            kokoro(&v).voices_file.as_deref(),
            Some("the-painter-voices.bin")
        );
    }

    #[test]
    fn a_host_on_elevenlabs_keeps_a_kokoro_guest() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = setup(tmp.path());
        crate::voice::settings::store_override(
            &cfg,
            "malakor",
            Some(&VoiceChoice {
                provider: VoiceProvider::Elevenlabs,
                kokoro: None,
                elevenlabs: Some(ElevenlabsVoice {
                    voice_id: Some("abc123".into()),
                    model: None,
                    speed: None,
                }),
            }),
        )
        .unwrap();
        let host = crate::voice::settings::spoken_for(&cfg, "malakor");
        assert_eq!(host.provider, VoiceProvider::Elevenlabs);
        let guest = resolve(&cfg, "malakor", "the-painter").unwrap().unwrap();
        assert_eq!(guest.provider, VoiceProvider::Kokoro);
        assert!(matches!(guest.spec, Spec::Kokoro(_)));
    }
}
