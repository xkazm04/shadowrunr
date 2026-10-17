//! Mechanical verification of what the Reconciler proposes: evidence refs must resolve to a
//! real transcript line of a run in scope, and character diffs may touch only `bible.md` or
//! `tones/*.md`, never `constitution.md`.

use std::path::Path;

use crate::runs::RefIndex;
use crate::types::{DroppedItem, Evidence};

/// Keep only refs that resolve; drop evidence left with no ref. Every drop is recorded.
pub fn verify_evidence(
    index: &RefIndex<'_>,
    items: Vec<Evidence>,
    dropped: &mut Vec<DroppedItem>,
) -> Vec<Evidence> {
    let mut out = Vec::new();
    for ev in items {
        let claim = ev.claim.trim().to_string();
        if claim.is_empty() {
            dropped.push(DroppedItem {
                what: "evidence".into(),
                label: String::new(),
                value: None,
                reason: "empty claim".into(),
            });
            continue;
        }
        let mut refs: Vec<String> = Vec::new();
        for r in &ev.refs {
            match index.resolve(r) {
                Ok(canon) => {
                    if !refs.contains(&canon) {
                        refs.push(canon);
                    }
                }
                Err(reason) => dropped.push(DroppedItem {
                    what: "evidence_ref".into(),
                    label: claim.clone(),
                    value: Some(r.clone()),
                    reason,
                }),
            }
        }
        if refs.is_empty() {
            dropped.push(DroppedItem {
                what: "evidence".into(),
                label: claim,
                value: None,
                reason: "no evidence ref resolved against the transcripts".into(),
            });
            continue;
        }
        out.push(Evidence { claim, refs });
    }
    out
}

#[derive(Debug, Default)]
struct FilePatch {
    old: String,
    new: String,
    /// Old-side lines (context + removed) of each hunk.
    hunks: Vec<Vec<String>>,
}

fn header_path(rest: &str) -> String {
    rest.split('\t').next().unwrap_or("").trim().to_string()
}

/// Normalise a diff path to the character package: strip `a/` `b/`, `./` and a leading
/// `characters/<id>/`. `Err` for absolute or parent-escaping paths.
fn package_path(raw: &str) -> Result<String, String> {
    let p = raw.replace('\\', "/");
    let p = p
        .strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(&p);
    let p = p.trim_start_matches("./");
    let p = match p.strip_prefix("characters/") {
        Some(rest) => rest.split_once('/').map(|(_, r)| r).unwrap_or(rest),
        None => p,
    };
    if p.starts_with('/') || p.contains(':') || p.split('/').any(|c| c == "..") {
        return Err(format!("path {raw:?} escapes the character package"));
    }
    Ok(p.to_string())
}

fn is_constitution(path: &str) -> bool {
    path.to_ascii_lowercase().contains("constitution.md")
}

fn allowed(path: &str) -> bool {
    if path == "bible.md" {
        return true;
    }
    match path.strip_prefix("tones/") {
        Some(name) => {
            name.ends_with(".md") && name.len() > 3 && !name.contains('/') && !name.contains("..")
        }
        None => false,
    }
}

fn parse_counts(header: &str) -> Option<(usize, usize)> {
    // @@ -a[,b] +c[,d] @@
    let inner = header.strip_prefix("@@")?.split("@@").next()?.trim();
    let mut parts = inner.split_whitespace();
    let count = |s: &str, sign: char| -> Option<usize> {
        let s = s.strip_prefix(sign)?;
        match s.split_once(',') {
            Some((_, n)) => n.parse().ok(),
            None => s.parse::<usize>().ok().map(|_| 1),
        }
    };
    Some((count(parts.next()?, '-')?, count(parts.next()?, '+')?))
}

fn parse_diff(diff: &str) -> Result<Vec<FilePatch>, String> {
    let lines: Vec<&str> = diff.lines().collect();
    let mut files: Vec<FilePatch> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let (Some(old), Some(new)) = (
            line.strip_prefix("--- "),
            lines.get(i + 1).and_then(|l| l.strip_prefix("+++ ")),
        ) {
            files.push(FilePatch {
                old: header_path(old),
                new: header_path(new),
                hunks: Vec::new(),
            });
            i += 2;
            continue;
        }
        if line.starts_with("@@") {
            let Some(file) = files.last_mut() else {
                return Err("hunk before any ---/+++ file header".into());
            };
            let (mut old_left, mut new_left) =
                parse_counts(line).ok_or_else(|| format!("malformed hunk header {line:?}"))?;
            let mut old_side = Vec::new();
            i += 1;
            while old_left > 0 || new_left > 0 {
                let Some(l) = lines.get(i) else {
                    return Err("hunk is shorter than its header says".into());
                };
                if let Some(rest) = l.strip_prefix('-') {
                    old_left = old_left
                        .checked_sub(1)
                        .ok_or("hunk removes more lines than its header says")?;
                    old_side.push(rest.to_string());
                } else if l.starts_with('+') {
                    new_left = new_left
                        .checked_sub(1)
                        .ok_or("hunk adds more lines than its header says")?;
                } else if l.starts_with('\\') {
                    // "\ No newline at end of file"
                } else {
                    let rest = l.strip_prefix(' ').unwrap_or(l);
                    if !(l.starts_with(' ') || l.is_empty()) {
                        return Err(format!("malformed hunk line {l:?}"));
                    }
                    old_left = old_left
                        .checked_sub(1)
                        .ok_or("hunk has more context than its header says")?;
                    new_left = new_left
                        .checked_sub(1)
                        .ok_or("hunk has more context than its header says")?;
                    old_side.push(rest.to_string());
                }
                i += 1;
            }
            file.hunks.push(old_side);
            continue;
        }
        if is_constitution(line) {
            return Err("diff header names constitution.md, which is immutable".into());
        }
        i += 1;
    }
    Ok(files)
}

/// Validate a `character_change` diff. With `character_dir`, also check that every hunk's
/// old side matches the current file, so the owner is never shown a diff that cannot apply.
pub fn check_diff(diff: &str, character_dir: Option<&Path>) -> Result<(), String> {
    let files = parse_diff(diff)?;
    if files.is_empty() {
        return Err("not a unified diff: no ---/+++ file header".into());
    }
    for f in &files {
        for raw in [&f.old, &f.new] {
            if is_constitution(raw) {
                return Err("diff touches constitution.md, which is immutable".into());
            }
        }
        if f.new == "/dev/null" {
            return Err(format!("diff deletes {}; deletions are not allowed", f.old));
        }
        let new = package_path(&f.new)?;
        if !allowed(&new) {
            return Err(format!(
                "diff may only touch bible.md or tones/*.md, not {new}"
            ));
        }
        let creates = f.old == "/dev/null";
        if !creates {
            let old = package_path(&f.old)?;
            if old != new {
                return Err(format!(
                    "diff renames {old} to {new}; renames are not allowed"
                ));
            }
        }
        if f.hunks.is_empty() {
            return Err(format!("diff for {new} has no hunks"));
        }
        let Some(dir) = character_dir else { continue };
        let path = dir.join(&new);
        if creates {
            if path.exists() {
                return Err(format!("diff creates {new}, which already exists"));
            }
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {new} to check the diff: {e}"))?;
        let current: Vec<&str> = text.lines().map(|l| l.trim_end()).collect();
        for (h, old_side) in f.hunks.iter().enumerate() {
            if old_side.is_empty() {
                continue;
            }
            let want: Vec<&str> = old_side.iter().map(|l| l.trim_end()).collect();
            let found = current.windows(want.len()).any(|w| w == want.as_slice());
            if !found {
                return Err(format!(
                    "hunk {} of {new} does not match the current file",
                    h + 1
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIBLE_DIFF: &str = "--- a/bible.md\n+++ b/bible.md\n@@ -1,2 +1,3 @@\n # Bible\n-old line\n+new line\n+another\n";

    #[test]
    fn bible_and_tone_diffs_pass_structure_checks() {
        assert_eq!(check_diff(BIBLE_DIFF, None), Ok(()));
        let tone = "--- a/characters/malakor/tones/theatrical.md\n+++ b/characters/malakor/tones/theatrical.md\n@@ -3 +3 @@\n-x\n+y\n";
        assert_eq!(check_diff(tone, None), Ok(()));
    }

    #[test]
    fn constitution_and_other_paths_are_rejected() {
        let c = "--- a/constitution.md\n+++ b/constitution.md\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(check_diff(c, None).unwrap_err().contains("constitution"));
        let c2 = "--- a/characters/malakor/constitution.md\n+++ b/characters/malakor/constitution.md\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(check_diff(c2, None).unwrap_err().contains("constitution"));
        let rename = "diff --git a/bible.md b/constitution.md\nrename to constitution.md\n";
        assert!(check_diff(rename, None)
            .unwrap_err()
            .contains("constitution"));
        let other = "--- a/character.toml\n+++ b/character.toml\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(check_diff(other, None).is_err());
        let escape =
            "--- a/tones/../constitution.md\n+++ b/tones/../constitution.md\n@@ -1 +1 @@\n-a\n+b\n";
        assert!(check_diff(escape, None).is_err());
        assert!(check_diff("just prose", None).is_err());
    }

    #[test]
    fn hunks_must_match_the_current_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bible.md"), "# Bible\nold line\nmore\n").unwrap();
        assert_eq!(check_diff(BIBLE_DIFF, Some(dir.path())), Ok(()));
        std::fs::write(dir.path().join("bible.md"), "# Bible\nchanged\n").unwrap();
        assert!(check_diff(BIBLE_DIFF, Some(dir.path()))
            .unwrap_err()
            .contains("does not match"));
    }
}
