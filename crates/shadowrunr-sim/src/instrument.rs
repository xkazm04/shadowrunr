//! Instrument identity. A run is only comparable with runs made by the same instrument: the
//! same character package, tone file, segment tables, judge rubric, calibration and sim logic.
//! Results from different hashes are never merged.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::SimError;

/// Bump when the simulator's or the judge's logic changes in a way that changes verdicts
/// (prompt templates, verification rules, scheduling).
pub const SIM_LOGIC_VERSION: &str = "shadowrunr-sim/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentFile {
    /// Logical path, e.g. `character/constitution.md`, `tone/theatrical.md`.
    pub path: String,
    /// sha256 of the LF-normalised content, or `absent`.
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    /// Everything except the tone: one per tournament.
    pub base_hash: String,
    /// Base plus the tone file: one per run, the hash results are grouped by.
    pub instrument_hash: String,
    pub sim_logic: String,
    pub files: Vec<InstrumentFile>,
}

fn sha_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// LF-normalised so a Windows checkout hashes like a Unix one.
fn norm(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn file_entry(logical: String, path: &Path) -> Result<InstrumentFile, SimError> {
    let sha256 = match std::fs::read(path) {
        Ok(b) => sha_hex(&norm(&b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "absent".to_string(),
        Err(e) => return Err(SimError::Io(format!("{}: {e}", path.display()))),
    };
    Ok(InstrumentFile {
        path: logical,
        sha256,
    })
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), SimError> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let p = e.path();
        if p.is_dir() {
            walk(&p, &child_rel, out)?;
        } else {
            out.push((child_rel, p));
        }
    }
    Ok(())
}

fn combine(files: &[InstrumentFile], logic: &str) -> String {
    let mut h = Sha256::new();
    h.update(logic.as_bytes());
    for f in files {
        h.update(b"\n");
        h.update(f.path.as_bytes());
        h.update(b"=");
        h.update(f.sha256.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub struct InstrumentInputs<'a> {
    /// `characters/<id>/`
    pub character_dir: &'a Path,
    pub tone: &'a str,
    /// Segment tables (`crates/shadowrunr-segments/data`).
    pub segments_dir: &'a Path,
    pub rubric_path: &'a Path,
    /// Calibration text actually in use (the file or the compiled-in default).
    pub calibration_text: &'a str,
}

pub fn compute(inp: &InstrumentInputs<'_>) -> Result<Instrument, SimError> {
    let mut files = Vec::new();
    let cd = inp.character_dir;
    for name in [
        "character.toml",
        "constitution.md",
        "bible.md",
        "situations.json",
    ] {
        files.push(file_entry(format!("character/{name}"), &cd.join(name))?);
    }
    let mut foils = Vec::new();
    walk(&cd.join("foils"), "", &mut foils)?;
    for (rel, p) in foils {
        files.push(file_entry(format!("character/foils/{rel}"), &p)?);
    }
    let mut seg = Vec::new();
    walk(inp.segments_dir, "", &mut seg)?;
    for (rel, p) in seg {
        files.push(file_entry(format!("segments/{rel}"), &p)?);
    }
    files.push(file_entry("rubric.toml".into(), inp.rubric_path)?);
    files.push(InstrumentFile {
        path: "calibration.json".into(),
        sha256: sha_hex(&norm(inp.calibration_text.as_bytes())),
    });
    let base_hash = combine(&files, SIM_LOGIC_VERSION);

    let tone = file_entry(
        format!("tone/{}.md", inp.tone),
        &cd.join("tones").join(format!("{}.md", inp.tone)),
    )?;
    let instrument_hash = combine(std::slice::from_ref(&tone), &base_hash);
    files.push(tone);
    Ok(Instrument {
        base_hash,
        instrument_hash,
        sim_logic: SIM_LOGIC_VERSION.to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(root: &Path) {
        let c = root.join("char");
        std::fs::create_dir_all(c.join("tones")).unwrap();
        std::fs::create_dir_all(c.join("foils")).unwrap();
        std::fs::write(c.join("character.toml"), "id = \"x\"\n").unwrap();
        std::fs::write(c.join("constitution.md"), "rules\r\n").unwrap();
        std::fs::write(c.join("situations.json"), "{}").unwrap();
        std::fs::write(c.join("tones/a.md"), "tone a").unwrap();
        std::fs::write(c.join("tones/b.md"), "tone b").unwrap();
        std::fs::write(c.join("foils/f.md"), "foil").unwrap();
        std::fs::create_dir_all(root.join("seg")).unwrap();
        std::fs::write(root.join("seg/ranks.toml"), "r").unwrap();
        std::fs::write(root.join("rubric.toml"), "v = 1").unwrap();
    }

    fn inst(root: &Path, tone: &str) -> Instrument {
        compute(&InstrumentInputs {
            character_dir: &root.join("char"),
            tone,
            segments_dir: &root.join("seg"),
            rubric_path: &root.join("rubric.toml"),
            calibration_text: "{}",
        })
        .unwrap()
    }

    #[test]
    fn hash_tracks_every_input() {
        let d = tempfile::tempdir().unwrap();
        fixture(d.path());
        let a = inst(d.path(), "a");
        let b = inst(d.path(), "b");
        assert_eq!(a.base_hash, b.base_hash, "tone is not in the base");
        assert_ne!(a.instrument_hash, b.instrument_hash);
        assert_eq!(
            a.instrument_hash,
            inst(d.path(), "a").instrument_hash,
            "stable"
        );

        // CRLF vs LF does not change the hash.
        std::fs::write(d.path().join("char/constitution.md"), "rules\n").unwrap();
        assert_eq!(inst(d.path(), "a").instrument_hash, a.instrument_hash);

        for (file, content) in [
            ("rubric.toml", "v = 2"),
            ("seg/ranks.toml", "r2"),
            ("char/foils/f.md", "foil2"),
            ("char/tones/a.md", "tone a2"),
        ] {
            let before = inst(d.path(), "a").instrument_hash;
            std::fs::write(d.path().join(file), content).unwrap();
            assert_ne!(inst(d.path(), "a").instrument_hash, before, "{file}");
        }
    }
}
