//! Instrument identity. A run is only comparable with runs made by the same instrument: the
//! same character package (including its optional rules and the effective enabled set after
//! the operator's override), tone file, segment tables, judge rubric, calibration, installed
//! moderation lists and sim logic. Results from different hashes are never merged.
//!
//! WP15: when a run carries the Lord's brief, the brief's canonical JSON digest joins the
//! base hash as `brief.json`, so tournaments with and without a brief (or with different
//! briefs) never mix. With no brief nothing is added, so every existing hash is unchanged.
//!
//! Reconcile opt-5: an experiment a run turns on joins the base hash as `experiment/<name>`,
//! so a treatment never mixes with its control. With none on nothing is added.
//!
//! A staged real case joins the base hash as `case/<id>` with its content hash, so runs on
//! different cases (or on a case whose records changed) never mix. The fictional demo case
//! is already hashed with the segment data, so a run without a case adds nothing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shadowrunr_types::reconcile::StreamBrief;

use crate::SimError;

/// Bump when the simulator's or the judge's logic changes in a way that changes verdicts
/// (prompt templates, verification rules, scheduling).
pub const SIM_LOGIC_VERSION: &str = "shadowrunr-sim/5";

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

/// sha256 of the brief's canonical JSON (serde's field order, compact; the builder fixes the
/// directive order), the digest the instrument and `--plan-only` show.
pub fn brief_digest(b: &StreamBrief) -> String {
    sha_hex(&serde_json::to_vec(b).unwrap_or_default())
}

/// Logical path of the brief entry in the instrument's file list.
pub const BRIEF_ENTRY: &str = "brief.json";

/// Logical path prefix of an experiment entry (`experiment/rate_cap_exemptions`).
pub const EXPERIMENT_PREFIX: &str = "experiment/";

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
    /// The run's brief, when it has one (WP15).
    pub brief: Option<&'a StreamBrief>,
    /// Named experiments the run turns on (`SimOptions::experiments`): each joins the base
    /// hash as `experiment/<name>`. None (the default) adds nothing.
    pub experiments: &'a [&'a str],
    /// The staged real case the run audits: `(case_id, content_sha256)`.
    pub case: Option<(&'a str, &'a str)>,
}

/// Logical path prefix of a staged real case's entry (`case/<id>`).
pub const CASE_PREFIX: &str = "case/";

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
    let mut rules = Vec::new();
    walk(&cd.join("rules"), "", &mut rules)?;
    for (rel, p) in rules {
        files.push(file_entry(format!("character/rules/{rel}"), &p)?);
    }
    // The effective enabled set: package defaults with the operator's override applied (the
    // brain reads the override by the same convention, `RulesOverride::Auto`).
    let enabled = shadowrunr_brain::rules::load_rules(cd, &shadowrunr_brain::RulesOverride::Auto)
        .map_err(|e| SimError::Spec(format!("character rules: {e}")))?;
    files.push(InstrumentFile {
        path: "character/rules.enabled".into(),
        sha256: sha_hex(shadowrunr_brain::rules::enabled_set_text(&enabled).as_bytes()),
    });
    // Operator-installed moderation lists the brain compiles (`<repo>/policy/local/*.txt`,
    // beside the `platform-rules.json` that `PolicySource::Auto` reads).
    let mut lists = Vec::new();
    walk(
        &cd.join("..").join("..").join("policy").join("local"),
        "",
        &mut lists,
    )?;
    for (rel, p) in lists {
        if rel.ends_with(".txt") {
            files.push(file_entry(format!("policy/local/{rel}"), &p)?);
        }
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
    if let Some(b) = inp.brief {
        files.push(InstrumentFile {
            path: BRIEF_ENTRY.into(),
            sha256: brief_digest(b),
        });
    }
    for name in inp.experiments {
        files.push(InstrumentFile {
            path: format!("{EXPERIMENT_PREFIX}{name}"),
            sha256: sha_hex(b"on"),
        });
    }
    if let Some((id, sha)) = inp.case {
        files.push(InstrumentFile {
            path: format!("{CASE_PREFIX}{id}"),
            sha256: sha.to_string(),
        });
    }
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
            brief: None,
            experiments: &[],
            case: None,
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

    #[test]
    fn hash_tracks_rules_the_enabled_set_and_installed_lists() {
        // A repo layout, so the Auto override (`<repo>/data/...`) and `<repo>/policy/local`
        // resolve inside the temp dir.
        let d = tempfile::tempdir().unwrap();
        let repo = d.path().join("repo");
        let c = repo.join("characters").join("x");
        std::fs::create_dir_all(c.join("tones")).unwrap();
        std::fs::create_dir_all(c.join("rules")).unwrap();
        std::fs::write(
            c.join("character.toml"),
            "id = \"x\"\n[rules]\nalpha = true\n",
        )
        .unwrap();
        std::fs::write(c.join("constitution.md"), "rules").unwrap();
        std::fs::write(c.join("tones/a.md"), "tone a").unwrap();
        std::fs::write(c.join("rules/alpha.md"), "# Alpha\n\nDo alpha.").unwrap();
        std::fs::write(d.path().join("rubric.toml"), "v = 1").unwrap();
        let h = || {
            compute(&InstrumentInputs {
                character_dir: &c,
                tone: "a",
                segments_dir: &d.path().join("seg"),
                rubric_path: &d.path().join("rubric.toml"),
                calibration_text: "{}",
                brief: None,
                experiments: &[],
                case: None,
            })
            .unwrap()
            .instrument_hash
        };
        let base = h();
        assert_eq!(h(), base, "stable");

        // The rule text is hashed.
        std::fs::write(c.join("rules/alpha.md"), "# Alpha\n\nDo alpha twice.").unwrap();
        let edited = h();
        assert_ne!(edited, base);

        // The operator's override changes the effective set, and so the hash; setting it
        // back to the default restores it.
        let over = repo.join("data/characters/x/rules.json");
        std::fs::create_dir_all(over.parent().unwrap()).unwrap();
        std::fs::write(&over, r#"{"alpha": false}"#).unwrap();
        let toggled = h();
        assert_ne!(toggled, edited);
        std::fs::write(&over, r#"{"alpha": true}"#).unwrap();
        assert_eq!(h(), edited);

        // An installed list's content is hashed; its metadata json is not needed.
        let local = repo.join("policy/local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("fixture.txt"), "zorblax\n").unwrap();
        let with_list = h();
        assert_ne!(with_list, edited);
        std::fs::write(local.join("fixture.txt"), "zorblax\nquexfin\n").unwrap();
        assert_ne!(h(), with_list);
        std::fs::write(local.join("fixture.json"), "{}").unwrap();
        let i = compute(&InstrumentInputs {
            character_dir: &c,
            tone: "a",
            segments_dir: &d.path().join("seg"),
            rubric_path: &d.path().join("rubric.toml"),
            calibration_text: "{}",
            brief: None,
            experiments: &[],
            case: None,
        })
        .unwrap();
        assert!(i.files.iter().any(|f| f.path == "policy/local/fixture.txt"));
        assert!(!i
            .files
            .iter()
            .any(|f| f.path == "policy/local/fixture.json"));
        assert!(i.files.iter().any(|f| f.path == "character/rules/alpha.md"));
    }

    fn brief(text: &str) -> StreamBrief {
        StreamBrief {
            lord_dispatch: Some("Audit the sock bureau.".into()),
            segments: Vec::new(),
            topics: Vec::new(),
            directives: vec![shadowrunr_types::reconcile::Directive {
                id: "dir-1".into(),
                kind: shadowrunr_types::reconcile::DirectiveKind::Prevent,
                text: text.into(),
                scope: None,
                created_ms: 1,
                active: true,
                proposed: false,
            }],
        }
    }

    fn inst_with(root: &Path, b: Option<&StreamBrief>) -> Instrument {
        compute(&InstrumentInputs {
            character_dir: &root.join("char"),
            tone: "a",
            segments_dir: &root.join("seg"),
            rubric_path: &root.join("rubric.toml"),
            calibration_text: "{}",
            brief: b,
            experiments: &[],
            case: None,
        })
        .unwrap()
    }

    /// `fixture` hashed with no brief and no experiment: the golden that both entries must
    /// leave untouched when absent. It moves only with `SIM_LOGIC_VERSION` (`/5`: carried bait
    /// picked first, after `/4`: two picks a beat, gifts and the cold open carried, the
    /// speech-aware say gap, a full-length session).
    const PRE_BRIEF_HASH: &str = "c6abc7221950c0f7d4859e3e2c96fefa21416e5492c5c2864fda283c4996d65c";

    #[test]
    fn a_brief_joins_the_hash_only_when_present() {
        // Nested two levels, so the override and installed-list lookups (`<char>/../..`)
        // stay inside the temp dir and the golden does not depend on the machine.
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("repo").join("characters");
        fixture(&root);
        let none = inst_with(&root, None);
        assert_eq!(
            none.instrument_hash, PRE_BRIEF_HASH,
            "no brief: hash unchanged"
        );
        assert!(!none.files.iter().any(|f| f.path == BRIEF_ENTRY));

        let b = brief("No coin bits.");
        let with = inst_with(&root, Some(&b));
        assert_ne!(
            with.base_hash, none.base_hash,
            "a brief starts a new tournament"
        );
        assert_ne!(with.instrument_hash, none.instrument_hash);
        let entry = with.files.iter().find(|f| f.path == BRIEF_ENTRY).unwrap();
        assert_eq!(entry.sha256, brief_digest(&b));
        assert_eq!(
            inst_with(&root, Some(&b)).instrument_hash,
            with.instrument_hash,
            "stable"
        );
        let other = inst_with(&root, Some(&brief("No coin bits, ever.")));
        assert_ne!(
            other.instrument_hash, with.instrument_hash,
            "briefs never mix"
        );
    }

    #[test]
    fn an_experiment_joins_the_hash_only_when_on() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("repo").join("characters");
        fixture(&root);
        let off = inst_with(&root, None);
        assert_eq!(off.instrument_hash, PRE_BRIEF_HASH, "off: hash unchanged");
        let on = compute(&InstrumentInputs {
            character_dir: &root.join("char"),
            tone: "a",
            segments_dir: &root.join("seg"),
            rubric_path: &root.join("rubric.toml"),
            calibration_text: "{}",
            brief: None,
            experiments: &["rate_cap_exemptions"],
            case: None,
        })
        .unwrap();
        assert_ne!(
            on.base_hash, off.base_hash,
            "treatment and control never mix"
        );
        assert!(on
            .files
            .iter()
            .any(|f| f.path == "experiment/rate_cap_exemptions"));
    }
}
