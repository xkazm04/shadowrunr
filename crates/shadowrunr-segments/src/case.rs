//! SEG-02A cases: loading, validation and the fictional demo case.

use std::collections::BTreeSet;
use std::path::Path;

use shadowrunr_types::Case;

use crate::SegmentError;

/// The compiled-in fictional demo case: the exact bytes of `data/demo-case.json`.
pub const DEMO_CASE_JSON: &str = include_str!("../data/demo-case.json");

/// The fictional demo case (`fictional: true`). Every entity in it is invented.
pub fn demo_case() -> Case {
    parse_case("demo-case.json", DEMO_CASE_JSON).expect("built-in demo case is valid (tested)")
}

/// Parse and validate a case from JSON text.
pub fn parse_case(file: &str, text: &str) -> Result<Case, SegmentError> {
    let case: Case = serde_json::from_str(text).map_err(|e| SegmentError::Data {
        file: file.into(),
        detail: e.to_string(),
    })?;
    validate_case(&case)?;
    Ok(case)
}

pub fn load_case(path: &Path) -> Result<Case, SegmentError> {
    let text = std::fs::read_to_string(path).map_err(|e| SegmentError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    parse_case(&path.display().to_string(), &text)
}

/// Hex ids are unique, coordinates are distinct, and links join existing hexes.
pub fn validate_case(case: &Case) -> Result<(), SegmentError> {
    let bad = |detail: String| SegmentError::Config {
        detail: format!("case {}: {detail}", case.id),
    };
    if case.hexes.is_empty() {
        return Err(bad("no hexes".into()));
    }
    let mut ids = BTreeSet::new();
    let mut coords = BTreeSet::new();
    for h in &case.hexes {
        if !ids.insert(h.id.as_str()) {
            return Err(bad(format!("duplicate hex id {}", h.id)));
        }
        if !coords.insert((h.q, h.r)) {
            return Err(bad(format!("two hexes at ({}, {})", h.q, h.r)));
        }
    }
    for (a, b) in &case.links {
        if !ids.contains(a.as_str()) || !ids.contains(b.as_str()) || a == b {
            return Err(bad(format!("bad link {a} - {b}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_case_is_fictional_and_sized() {
        let c = demo_case();
        assert!(c.fictional);
        assert!((12..=20).contains(&c.hexes.len()), "{}", c.hexes.len());
        assert!(!c.links.is_empty());
        assert!(c.title.contains("FICTIONAL"));
        // Starts fogged with no grades, and no source points at a real domain.
        for h in &c.hexes {
            assert!(h.fog && h.grade.is_none(), "{}", h.id);
            for s in &h.sources {
                let host = s.trim_start_matches("https://").split('/').next().unwrap();
                assert!(host.ends_with(".example"), "{s}");
            }
        }
    }

    #[test]
    fn broken_cases_rejected() {
        let dup = DEMO_CASE_JSON.replace("\"id\": \"HEX-02\"", "\"id\": \"HEX-01\"");
        assert!(parse_case("x", &dup).is_err());
        let bad_link =
            DEMO_CASE_JSON.replace("[\"HEX-01\", \"HEX-02\"]", "[\"HEX-01\", \"HEX-99\"]");
        assert!(parse_case("x", &bad_link).is_err());
    }
}
