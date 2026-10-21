//! SEG-02A cases: loading, validation and the fictional demo case.

use std::collections::BTreeSet;
use std::path::Path;

use shadowrunr_types::Case;

use crate::research::is_day;
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

/// Hex ids are unique, coordinates are distinct, and links join existing hexes. A hex's
/// `date` is a `YYYY-MM-DD` day, a `thesis` is not blank, and the `acts` (the evidence board's
/// regions) have names and place each hex of the case at most once.
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
        if h.date.as_deref().is_some_and(|d| !is_day(d)) {
            return Err(bad(format!("{}: date must be YYYY-MM-DD", h.id)));
        }
    }
    for (a, b) in &case.links {
        if !ids.contains(a.as_str()) || !ids.contains(b.as_str()) || a == b {
            return Err(bad(format!("bad link {a} - {b}")));
        }
    }
    if case.thesis.as_deref().is_some_and(|t| t.trim().is_empty()) {
        return Err(bad("blank thesis".into()));
    }
    let mut placed = BTreeSet::new();
    for act in &case.acts {
        if act.name.trim().is_empty() {
            return Err(bad("an act without a name".into()));
        }
        for id in &act.hexes {
            if !ids.contains(id.as_str()) {
                return Err(bad(format!("act {:?} names unknown hex {id}", act.name)));
            }
            if !placed.insert(id.as_str()) {
                return Err(bad(format!("{id} is in two acts")));
            }
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
    fn the_demo_case_tells_its_story_on_the_board() {
        let c = demo_case();
        assert!(c.thesis.as_deref().is_some_and(|t| t.contains("4%")));
        // Every card has a role and a date, and sits in exactly one act.
        for h in &c.hexes {
            assert!(h.role.is_some() && h.date.is_some(), "{}", h.id);
            let acts = c.acts.iter().filter(|a| a.hexes.contains(&h.id)).count();
            assert_eq!(acts, 1, "{}", h.id);
        }
        let names: Vec<&str> = c.acts.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "The 4% promise",
                "Frozen withdrawals and the money trail",
                "Shells, the regulator and the old crew"
            ]
        );
        // The roles the story needs are all on the board.
        use shadowrunr_types::HexRole::*;
        for role in [
            Victim, Promoter, Shell, Processor, Regulator, Claim, Wallet, Exchange, Company,
        ] {
            assert!(c.hexes.iter().any(|h| h.role == Some(role)), "{role:?}");
        }
    }

    #[test]
    fn broken_board_fields_rejected() {
        let mut bad_date = demo_case();
        bad_date.hexes[0].date = Some("2026-4-2".into());
        let mut blank_thesis = demo_case();
        blank_thesis.thesis = Some(" ".into());
        let mut unknown = demo_case();
        unknown.acts[0].hexes.push("HEX-99".into());
        let mut twice = demo_case();
        twice.acts[1].hexes.push("HEX-01".into());
        let mut unnamed = demo_case();
        unnamed.acts[2].name = String::new();
        for c in [bad_date, blank_thesis, unknown, twice, unnamed] {
            assert!(validate_case(&c).is_err(), "{:?}", c.acts);
        }
        // Both are optional: a case without them is valid.
        let mut bare = demo_case();
        bare.thesis = None;
        bare.acts.clear();
        for h in &mut bare.hexes {
            h.role = None;
            h.date = None;
        }
        assert!(validate_case(&bare).is_ok());
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
