//! Package documents for the operator's agent (docs/spec/contract.md, appendix "athena
//! copilot"): `character.doc.get` reads one whole package document and `character.doc.set`
//! replaces it.
//!
//! - `doc` comes from a closed list ([`check_doc`]) and is checked before any IO.
//! - The character comes through [`tone::package_dir`] and every existing target is checked to
//!   resolve inside `paths.characters`, as the Tone Lab writes are.
//! - A write is refused with `conflict` unless the file still has `base_digest` (`""`: the file
//!   must not exist). The previous bytes are copied to
//!   `<data>/character-backups/<id>/<unix_ms>-<doc, "/" as "__">`, then the text goes through
//!   [`write_validated`] under [`WRITE_LOCK`]: a write that adds an error is undone and refused.
//!   Nothing is normalised, so the digest a `get` returns is the digest of the text it returns.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use shadowrunr_types::catalog::{
    CharacterDoc, CharacterDocGetParams, CharacterDocSetParams, CharacterDocSetResult,
};
use shadowrunr_types::ErrorCode;

use crate::config::Config;
use crate::rpc::RpcError;
use crate::server::AppState;
use crate::tone::{self, contained, io, write_validated, WRITE_LOCK};

/// The package documents the operator's agent may read and replace, besides `foils/<slug>.md`.
pub const DOCS: [&str; 6] = [
    "constitution.md",
    "bible.md",
    "directions.md",
    "character.toml",
    "stalls.toml",
    "situations.json",
];

/// The package dir holding the foils.
const FOILS_DIR: &str = "foils";
/// The longest foil slug.
const MAX_SLUG: usize = 40;
/// The backups dir under `[paths] data`.
const BACKUPS_DIR: &str = "character-backups";

/// `invalid_params` unless `doc` is on the closed list. Pure: no IO.
fn check_doc(doc: &str) -> Result<(), RpcError> {
    if DOCS.contains(&doc) {
        return Ok(());
    }
    if let Some(slug) = doc
        .strip_prefix("foils/")
        .and_then(|rest| rest.strip_suffix(".md"))
    {
        let plain = !slug.is_empty()
            && slug.len() <= MAX_SLUG
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        if plain {
            return Ok(());
        }
    }
    Err(RpcError::invalid(format!(
        "doc {doc:?} must be one of {} or foils/<slug>.md (slug [a-z0-9_-]{{1,{MAX_SLUG}}})",
        DOCS.join(", ")
    )))
}

/// `sha256:<lowercase hex>` of `bytes`.
pub fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// The package dir of `id` and the path of `doc` in it, checked to stay inside the characters
/// directory where it already exists. `doc` is checked first, before any IO.
fn target(cfg: &Config, id: &str, doc: &str) -> Result<(PathBuf, PathBuf), RpcError> {
    check_doc(doc)?;
    let dir = tone::package_dir(cfg, id)?;
    let mut path = dir.clone();
    for part in doc.split('/') {
        path.push(part);
    }
    if let Some(slug_dir) = doc.strip_prefix("foils/").map(|_| dir.join(FOILS_DIR)) {
        if slug_dir.exists() {
            contained(&cfg.paths.characters, &slug_dir)?;
        }
    }
    if path.exists() {
        contained(&cfg.paths.characters, &path)?;
    }
    Ok((dir, path))
}

/// The file's bytes; `None` without the file.
fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, RpcError> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Copy `bytes` to `<data>/character-backups/<id>/<unix_ms>-<doc, "/" as "__">` and return that
/// path relative to `data`, `/` separators. A name already taken (two writes in one ms) takes
/// the next ms.
fn backup(data: &Path, id: &str, doc: &str, bytes: &[u8]) -> Result<String, RpcError> {
    let dir = data.join(BACKUPS_DIR).join(id);
    std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let flat = doc.replace('/', "__");
    let mut ms = now_ms();
    loop {
        let name = format!("{ms}-{flat}");
        let path = dir.join(&name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(bytes)
                    .and_then(|()| f.sync_all())
                    .map_err(|e| io(&path, e))?;
                return Ok(format!("{BACKUPS_DIR}/{id}/{name}"));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => ms += 1,
            Err(e) => return Err(io(&path, e)),
        }
    }
}

pub fn get(cfg: &Config, p: &CharacterDocGetParams) -> Result<CharacterDoc, RpcError> {
    let (_, path) = target(cfg, &p.id, &p.doc)?;
    let Some(bytes) = read_bytes(&path)? else {
        return Err(RpcError::not_found(format!(
            "{} in character {}",
            p.doc, p.id
        )));
    };
    let digest = digest(&bytes);
    let text = String::from_utf8(bytes).map_err(|e| io(&path, e))?;
    Ok(CharacterDoc {
        id: p.id.clone(),
        doc: p.doc.clone(),
        text,
        digest,
    })
}

pub fn set(cfg: &Config, p: &CharacterDocSetParams) -> Result<CharacterDocSetResult, RpcError> {
    let (dir, path) = target(cfg, &p.id, &p.doc)?;
    let _held = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let current = read_bytes(&path)?;
    let previous_digest = current.as_deref().map(digest).unwrap_or_default();
    if previous_digest != p.base_digest {
        let message = match (&current, p.base_digest.is_empty()) {
            (Some(_), true) => format!(
                "{} already exists (digest {previous_digest}); read it and name its digest",
                p.doc
            ),
            (None, _) => format!(
                "{} does not exist; a new file takes base_digest \"\"",
                p.doc
            ),
            (Some(_), false) => format!(
                "{} changed since it was read (now {previous_digest}, not {})",
                p.doc, p.base_digest
            ),
        };
        return Err(RpcError::new(ErrorCode::Conflict, message));
    }
    let backup = match &current {
        Some(bytes) => Some(backup(&cfg.paths.data, &p.id, &p.doc, bytes)?),
        None => None,
    };
    write_validated(&dir, &path, &p.doc, &p.text)?;
    Ok(CharacterDocSetResult {
        id: p.id.clone(),
        doc: p.doc.clone(),
        digest: digest(p.text.as_bytes()),
        previous_digest,
        backup,
    })
}

pub async fn rpc_get(
    state: &Arc<AppState>,
    p: CharacterDocGetParams,
) -> Result<CharacterDoc, RpcError> {
    let cfg = state.cfg.clone();
    tone::blocking(move || get(&cfg, &p)).await
}

pub async fn rpc_set(
    state: &Arc<AppState>,
    p: CharacterDocSetParams,
) -> Result<CharacterDocSetResult, RpcError> {
    let cfg = state.cfg.clone();
    tone::blocking(move || set(&cfg, &p)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::e2e::{copy_dir, repo, test_config};

    /// A config whose characters dir is a temp copy of `malakor` (never the repo's files).
    fn setup(tmp: &Path) -> (Config, PathBuf) {
        let chars = tmp.join("characters");
        copy_dir(&repo().join("characters/malakor"), &chars.join("malakor"));
        let cfg = test_config(tmp, chars.clone());
        (cfg, chars.join("malakor"))
    }

    fn get_p(doc: &str) -> CharacterDocGetParams {
        CharacterDocGetParams {
            id: "malakor".into(),
            doc: doc.into(),
        }
    }

    fn set_p(doc: &str, text: &str, base: &str) -> CharacterDocSetParams {
        CharacterDocSetParams {
            id: "malakor".into(),
            doc: doc.into(),
            text: text.into(),
            base_digest: base.into(),
        }
    }

    #[test]
    fn digest_is_sha256_lowercase_hex_of_the_bytes() {
        assert_eq!(
            digest(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_doc_round_trips_and_keeps_a_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, dir) = setup(tmp.path());
        let before = get(&cfg, &get_p("bible.md")).unwrap();
        let raw = std::fs::read(dir.join("bible.md")).unwrap();
        assert_eq!(before.digest, digest(&raw));
        assert_eq!(before.text.as_bytes(), raw.as_slice(), "nothing normalised");

        let text = format!("{}\nOne more line from the operator.\n", before.text);
        let w = set(&cfg, &set_p("bible.md", &text, &before.digest)).unwrap();
        assert_eq!(w.previous_digest, before.digest);
        let after = get(&cfg, &get_p("bible.md")).unwrap();
        assert_eq!(after.text, text);
        assert_eq!(w.digest, after.digest);

        let rel = w.backup.clone().expect("an existing file is backed up");
        let name = rel
            .strip_prefix("character-backups/malakor/")
            .expect("relative to data, / separators");
        assert!(name.ends_with("-bible.md"), "{name}");
        let ms = name.strip_suffix("-bible.md").unwrap();
        assert!(ms.parse::<u128>().is_ok(), "{ms}");
        let mut on_disk = cfg.paths.data.clone();
        for part in rel.split('/') {
            on_disk.push(part);
        }
        assert_eq!(std::fs::read(on_disk).unwrap(), raw);

        let wire = serde_json::to_value(&w).unwrap();
        assert!(!crate::e2e::has_null(&wire));
    }

    #[test]
    fn a_foil_backup_flattens_its_path() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, _) = setup(tmp.path());
        let doc = "foils/the-inspector.md";
        let cur = get(&cfg, &get_p(doc)).unwrap();
        let w = set(
            &cfg,
            &set_p(doc, &cur.text.replace("Inspector", "Auditor"), &cur.digest),
        )
        .unwrap();
        let rel = w.backup.unwrap();
        assert!(rel.starts_with("character-backups/malakor/"), "{rel}");
        assert!(rel.ends_with("-foils__the-inspector.md"), "{rel}");
    }

    #[test]
    fn a_stale_digest_is_a_conflict_and_leaves_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, dir) = setup(tmp.path());
        let path = dir.join("directions.md");
        let raw = std::fs::read(&path).unwrap();
        let e = set(&cfg, &set_p("directions.md", "x\n", &digest(b"other"))).unwrap_err();
        assert_eq!(e.code, ErrorCode::Conflict, "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), raw);
        assert!(!cfg.paths.data.join(BACKUPS_DIR).exists(), "no backup");
    }

    #[test]
    fn an_empty_base_digest_creates_a_new_foil_and_is_refused_on_an_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, dir) = setup(tmp.path());
        let doc = "foils/the-archivist_2.md";
        let e = get(&cfg, &get_p(doc)).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        let text = "# Foil: The Archivist\n\nAn original archetype.\n";
        let w = set(&cfg, &set_p(doc, text, "")).unwrap();
        assert_eq!(w.previous_digest, "");
        assert_eq!(w.backup, None);
        assert_eq!(
            serde_json::to_value(&w).unwrap().get("backup"),
            None,
            "an absent backup is omitted"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("foils").join("the-archivist_2.md")).unwrap(),
            text
        );
        assert_eq!(get(&cfg, &get_p(doc)).unwrap().digest, w.digest);

        let path = dir.join("bible.md");
        let raw = std::fs::read(&path).unwrap();
        let e = set(&cfg, &set_p("bible.md", "x\n", "")).unwrap_err();
        assert_eq!(e.code, ErrorCode::Conflict, "{e}");
        assert_eq!(std::fs::read(&path).unwrap(), raw);

        let e = set(&cfg, &set_p("foils/nobody.md", "x\n", &digest(b"x\n"))).unwrap_err();
        assert_eq!(
            e.code,
            ErrorCode::Conflict,
            "a missing file needs \"\": {e}"
        );
        assert!(!dir.join("foils").join("nobody.md").exists());
    }

    #[test]
    fn an_invalid_manifest_is_undone_with_its_issues() {
        let tmp = tempfile::tempdir().unwrap();
        let (cfg, dir) = setup(tmp.path());
        let path = dir.join("character.toml");
        let raw = std::fs::read(&path).unwrap();
        let cur = get(&cfg, &get_p("character.toml")).unwrap();
        let e = set(
            &cfg,
            &set_p("character.toml", "this is = [not toml\n", &cur.digest),
        )
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{e}");
        let detail = e.detail.clone().expect("a detail");
        let issues = detail
            .pointer("/issues")
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| panic!("detail.issues in {detail}"));
        assert!(!issues.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), raw, "the write was undone");
        assert_eq!(
            get(&cfg, &get_p("character.toml")).unwrap().digest,
            cur.digest
        );
    }

    #[test]
    fn a_doc_off_the_list_is_refused_before_any_io() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut cfg, _) = setup(tmp.path());
        // A characters dir that does not exist: any IO would fail with not_found or io.
        cfg.paths.characters = tmp.path().join("nowhere");
        for doc in [
            "../x",
            "../bible.md",
            "rules/x.md",
            "lines/default.toml",
            "/etc/passwd",
            "C:\\x.md",
            "foils\\a.md",
            "foils/A.md",
            "foils/.md",
            "foils/../bible.md",
            "foils/a/b.md",
            "foils/a.txt",
            "foils/a.md/",
            "./bible.md",
            "BIBLE.md",
            "",
            &format!("foils/{}.md", "a".repeat(41)),
        ] {
            let e = get(&cfg, &get_p(doc)).unwrap_err();
            assert_eq!(e.code, ErrorCode::InvalidParams, "get {doc:?}: {e}");
            let e = set(&cfg, &set_p(doc, "x", "")).unwrap_err();
            assert_eq!(e.code, ErrorCode::InvalidParams, "set {doc:?}: {e}");
        }
        assert!(check_doc(&format!("foils/{}.md", "a".repeat(40))).is_ok());
        for doc in DOCS {
            assert!(check_doc(doc).is_ok(), "{doc}");
        }
        assert!(!cfg.paths.data.join(BACKUPS_DIR).exists());
    }
}
