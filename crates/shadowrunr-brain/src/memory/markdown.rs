//! `---`-fenced frontmatter files: the memory's source of truth.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::util::fnv1a64;
use crate::BrainError;

/// Frontmatter fields (single-line values) plus a free-text body.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Doc {
    pub fields: BTreeMap<String, String>,
    pub body: String,
}

impl Doc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: impl ToString) -> &mut Self {
        let v = value.to_string().replace(['\n', '\r'], " ");
        self.fields.insert(key.to_string(), v);
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }

    pub fn req(&self, key: &str, path: &Path) -> Result<&str, BrainError> {
        self.get(key).ok_or_else(|| {
            BrainError::Store(format!("{}: missing frontmatter `{key}`", path.display()))
        })
    }

    pub fn num<T: std::str::FromStr>(&self, key: &str, path: &Path) -> Result<T, BrainError> {
        self.req(key, path)?.parse::<T>().map_err(|_| {
            BrainError::Store(format!(
                "{}: frontmatter `{key}` is not a number",
                path.display()
            ))
        })
    }

    pub fn render(&self) -> String {
        let mut s = String::from("---\n");
        for (k, v) in &self.fields {
            s.push_str(k);
            s.push_str(": ");
            s.push_str(v);
            s.push('\n');
        }
        s.push_str("---\n");
        s.push_str(&self.body);
        s.push('\n');
        s
    }

    pub fn parse(text: &str, path: &Path) -> Result<Doc, BrainError> {
        let text = text.replace("\r\n", "\n");
        let rest = text.strip_prefix("---\n").ok_or_else(|| {
            BrainError::Store(format!("{}: no frontmatter fence", path.display()))
        })?;
        let end = rest
            .find("\n---\n")
            .map(|i| (i, i + 5))
            .or_else(|| rest.strip_suffix("\n---").map(|r| (r.len(), rest.len())));
        let (fm_end, body_start) = end.ok_or_else(|| {
            BrainError::Store(format!("{}: unterminated frontmatter", path.display()))
        })?;
        let mut doc = Doc::new();
        for line in rest[..fm_end].lines() {
            if line.trim().is_empty() {
                continue;
            }
            let (k, v) = line
                .split_once(": ")
                .or_else(|| line.strip_suffix(':').map(|k| (k, "")))
                .ok_or_else(|| {
                    BrainError::Store(format!("{}: bad frontmatter line {line:?}", path.display()))
                })?;
            doc.fields.insert(k.trim().to_string(), v.to_string());
        }
        let body = &rest[body_start.min(rest.len())..];
        doc.body = body.strip_suffix('\n').unwrap_or(body).to_string();
        Ok(doc)
    }
}

pub fn write_doc(path: &Path, doc: &Doc) -> Result<String, BrainError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| BrainError::Store(format!("{}: {e}", parent.display())))?;
    }
    let text = doc.render();
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, &text).map_err(|e| BrainError::Store(format!("{}: {e}", tmp.display())))?;
    fs::rename(&tmp, path).map_err(|e| BrainError::Store(format!("{}: {e}", path.display())))?;
    Ok(content_hash(&text))
}

pub fn read_doc(path: &Path) -> Result<(Doc, String), BrainError> {
    let text = fs::read_to_string(path)
        .map_err(|e| BrainError::Store(format!("{}: {e}", path.display())))?;
    let text = text.replace("\r\n", "\n");
    Ok((Doc::parse(&text, path)?, content_hash(&text)))
}

pub fn content_hash(text: &str) -> String {
    format!("{:016x}", fnv1a64(text.as_bytes()))
}

/// A filesystem-safe path segment. Anything outside `[A-Za-z0-9._-]`, a leading `_` or `.`,
/// or a reserved name gets cleaned and suffixed with a hash of the original so two different
/// keys never share a file.
pub fn safe_segment(s: &str) -> String {
    let clean: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let reserved = clean.is_empty()
        || clean.starts_with('_')
        || clean.starts_with('.')
        || clean.len() > 80
        || is_windows_reserved(&clean);
    if clean == s && !reserved {
        clean
    } else {
        let short: String = clean
            .trim_start_matches(['_', '.'])
            .chars()
            .take(60)
            .collect();
        format!("k{short}-{:08x}", fnv1a64(s.as_bytes()) as u32)
    }
}

fn is_windows_reserved(s: &str) -> bool {
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

/// `<root>/<seg>/<seg>.md` for a slash-separated key.
pub fn key_path(root: &Path, key: &str) -> PathBuf {
    let segs: Vec<String> = key
        .split('/')
        .filter(|s| !s.is_empty())
        .map(safe_segment)
        .collect();
    let mut p = root.to_path_buf();
    let n = segs.len();
    for (i, s) in segs.into_iter().enumerate() {
        if i + 1 == n {
            p.push(format!("{s}.md"));
        } else {
            p.push(s);
        }
    }
    p
}

/// Every `*.md` under `dir`, recursively, sorted for deterministic rebuilds.
pub fn walk_md(dir: &Path) -> Result<Vec<PathBuf>, BrainError> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd =
            fs::read_dir(&d).map_err(|e| BrainError::Store(format!("{}: {e}", d.display())))?;
        for entry in rd {
            let p = entry
                .map_err(|e| BrainError::Store(format!("{}: {e}", d.display())))?
                .path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut d = Doc::new();
        d.set("id", "ep_1").set("importance", 3);
        d.body = "line one\n\n---\nline three".into();
        let text = d.render();
        let back = Doc::parse(&text, Path::new("x.md")).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn segments() {
        assert_eq!(safe_segment("v42"), "v42");
        assert_ne!(safe_segment("a b"), safe_segment("a_b"));
        assert!(safe_segment("..").starts_with('k'));
        assert!(safe_segment("_tombstones").starts_with('k'));
        assert!(safe_segment("CON").starts_with('k'));
        let p = key_path(Path::new("r"), "v42/lost_vote");
        assert_eq!(p, Path::new("r").join("v42").join("lost_vote.md"));
    }
}
