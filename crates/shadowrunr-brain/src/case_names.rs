//! Cards and records by name (HEX-NAMES): the case is spoken in the names the audience sees.
//!
//! The owner, on the recorded show sbx-20261022-135056-10m: "In detective segment referring to
//! hexes by ID is not very practical." He said "it pins the regulator to HEX-02", "DOC-01 is
//! the demo's order" and "Bone-clerks, to HEX-11 and lunarstake-help.example", while viewers see
//! named cards on the board and titled records in the browser. Ids are machine handles.
//!
//! Two layers, both deterministic:
//! - **The prompt speaks in names.** [`CaseNames::prompt_text`] re-renders the state summary,
//!   the working set and the case directive with each card and record name first and its id
//!   second in brackets (`Morrowgate Financial Conduct Office, regulator [HEX-13]`). The runtime's
//!   summary is not changed: the case loop ([`crate::orchestrate::case`]) still parses it as
//!   written. [`NAMES_RULE`] says the ids go only in action lines.
//! - **The output gate names what slipped through.** [`CaseNames::speak`] replaces a card or
//!   record id left in a spoken line with its spoken name from the live catalog ("that card" or
//!   "that record" for an unknown one), each rewrite counted as `id_spoken`. Op lines never pass
//!   through it.
//!
//! The catalog is read from the segment context every beat: the cards from its hexes (the label
//! as the board draws it, without the fictional marker the ribbon carries), the records from the
//! state summary's case-file line (every record filed so far, with its title).

use std::sync::OnceLock;

use regex::Regex;
use shadowrunr_types::{Hex, SegmentContext, SegmentId};

/// The output rule of a card or record id the gate replaced in a spoken line.
pub const ID_SPOKEN_RULE: &str = "id_spoken";
/// The [`ID_SPOKEN_RULE`] action.
pub const ID_REWRITTEN: &str = "rewritten";
/// The rule shown in a segment with case cards, after the segment's rules.
pub const NAMES_RULE: &str = "Speak in names: call a card by its name as the board shows it and \
a record by its title as the browser shows it. The ids in brackets ([HEX-13], [DOC-04]) go only \
in action lines (OP:) and are never spoken.";
/// The segment's name on screen (stage-kit `lore.ts`), said in SEG-02A.
pub const INVESTIGATION_LINE: &str =
    "This segment is The Investigation; call it that, or the case.";
/// Spoken in place of a card id the catalog does not know.
const THAT_CARD: &str = "that card";
/// Spoken in place of a record id the catalog does not know.
const THAT_RECORD: &str = "that record";
/// Record id prefixes (the evidence grades); a card id is `HEX-<n>`.
const RECORD_PREFIXES: &[&str] = &["DOC", "REP", "CLM", "SPC"];

/// One card or record.
#[derive(Debug, Clone, PartialEq)]
struct Named {
    id: String,
    /// The label or title as the data has it.
    raw: String,
    /// As the board or the browser shows it: without the fictional marker.
    display: String,
}

impl Named {
    fn new(id: &str, raw: &str) -> Self {
        Named {
            id: id.to_string(),
            raw: raw.to_string(),
            display: shown(raw),
        }
    }

    /// The name said aloud: the shown name up to its first comma ("Morrowgate Financial Conduct
    /// Office, regulator" -> "Morrowgate Financial Conduct Office").
    fn spoken(&self) -> &str {
        let s = self.display.split(", ").next().unwrap_or_default().trim();
        if s.is_empty() {
            &self.display
        } else {
            s
        }
    }
}

/// A label without its fictional marker (stage-kit `cardLabel`).
fn shown(label: &str) -> String {
    let lower = label.to_lowercase();
    let mut out = label.to_string();
    if let Some(at) = lower.find("(fictional)") {
        out.replace_range(at..at + "(fictional)".len(), " ");
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[A-Z][A-Z0-9]{1,5}-\d{1,4}\b").expect("valid"))
}

/// The case's cards and filed records by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaseNames {
    cards: Vec<Named>,
    records: Vec<Named>,
}

impl CaseNames {
    /// The catalog as the segment context shows it this beat.
    pub fn read(ctx: &SegmentContext) -> Self {
        let mut n = Self::from_hexes(&ctx.hexes);
        for line in ctx.state_summary.lines() {
            let Some(entries) = case_file_entries(line) else {
                continue;
            };
            for e in entries {
                if !n.records.iter().any(|r| r.id == e.record) {
                    n.records.push(Named::new(e.record, e.title));
                }
            }
        }
        n
    }

    /// The cards of a case, no records yet.
    pub fn from_hexes(hexes: &[Hex]) -> Self {
        CaseNames {
            cards: hexes.iter().map(|h| Named::new(&h.id, &h.label)).collect(),
            records: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    fn card(&self, id: &str) -> Option<&Named> {
        self.cards.iter().find(|c| c.id == id)
    }

    fn record(&self, id: &str) -> Option<&Named> {
        self.records.iter().find(|r| r.id == id)
    }

    fn any(&self, id: &str) -> Option<&Named> {
        self.card(id).or_else(|| self.record(id))
    }

    /// `id` as the prompt shows it: `Name [ID]`, or the bare id when unknown.
    pub fn tagged(&self, id: &str) -> String {
        match self.any(id) {
            Some(n) => format!("{} [{id}]", n.display),
            None => id.to_string(),
        }
    }

    /// The segment-context text a turn shows (the state summary, the working set): every known
    /// card and record name first, its id second in brackets. A card followed by its label in
    /// parentheses (a research report's `HEX-11 (label)`), the revealed line and the case-file
    /// line are re-ordered rather than named twice. Unchanged without a case.
    pub fn prompt_text(&self, text: &str) -> String {
        if self.is_empty() {
            return text.to_string();
        }
        let mut s = text.to_string();
        for c in &self.cards {
            s = s.replace(
                &format!("{} ({})", c.id, c.raw),
                &format!("{} [{}]", c.display, c.id),
            );
        }
        let lines: Vec<String> = s
            .split('\n')
            .map(|l| {
                let l = self.revealed_line(l).unwrap_or_else(|| l.to_string());
                let l = self.case_file_line(&l).unwrap_or(l);
                self.tag_bare(&l)
            })
            .collect();
        lines.join("\n")
    }

    /// Every bare known id (not already in brackets) as `Name [ID]`.
    fn tag_bare(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for m in id_regex().find_iter(text) {
            let bracketed = text[..m.start()].ends_with('[') && text[m.end()..].starts_with(']');
            if bracketed || self.any(m.as_str()).is_none() {
                continue;
            }
            out.push_str(&text[at..m.start()]);
            out.push_str(&self.tagged(m.as_str()));
            at = m.end();
        }
        out.push_str(&text[at..]);
        out
    }

    /// The runtime's `revealed n/m: HEX-01 REP ALLEGED: label; ...` with each card's name first.
    fn revealed_line(&self, line: &str) -> Option<String> {
        let (head, list) = line.split_once(": ")?;
        if !head.starts_with("revealed ") {
            return None;
        }
        let parts: Vec<String> = list
            .split("; ")
            .map(|e| {
                let (id, rest) = e.split_once(' ').unwrap_or((e, ""));
                match self.card(id) {
                    Some(c) => {
                        let grade = rest.strip_suffix(&c.raw).unwrap_or(rest);
                        let grade = grade.trim_end().trim_end_matches(':');
                        format!("{} [{id}] {grade}", c.display)
                            .trim_end()
                            .to_string()
                    }
                    None => e.to_string(),
                }
            })
            .collect();
        Some(format!("{head}: {}", parts.join("; ")))
    }

    /// The runtime's case-file line (`HEX-13 DOC-04 DOC: title; ...`) as `title [DOC-04] DOC, on
    /// card [HEX-13]`.
    fn case_file_line(&self, line: &str) -> Option<String> {
        let entries = case_file_entries(line)?;
        let (head, _) = line.split_once("): ")?;
        let parts: Vec<String> = entries
            .iter()
            .map(|e| {
                format!(
                    "{} [{}] {}, on {}",
                    shown(e.title),
                    e.record,
                    e.grade,
                    self.tagged(e.hex)
                )
            })
            .collect();
        Some(format!("{head}): {}", parts.join("; ")))
    }

    /// A spoken line with every card or record id replaced by its spoken name, and how many
    /// were replaced. An id beside the name already said (`HEX-11 and lunarstake-help.example`,
    /// `the regulator (HEX-13)`) is dropped rather than named twice; an unknown card or record
    /// id becomes "that card" or "that record"; any other `ABC-12` token, and an id in a chat
    /// command spelled out for viewers (`!hint HEX-05`), is left alone. Text from an `OP:` on is
    /// never touched.
    pub fn speak(&self, line: &str) -> (String, u32) {
        let (text, tail) = match line.find("OP:") {
            Some(at) => line.split_at(at),
            None => (line, ""),
        };
        let mut out = String::with_capacity(line.len());
        let mut at = 0;
        let mut n = 0;
        for m in id_regex().find_iter(text) {
            if m.start() < at {
                continue;
            }
            let id = m.as_str();
            let prefix = id.split('-').next().unwrap_or_default();
            let known = self.any(id);
            let is_record = self.record(id).is_some()
                || (self.card(id).is_none() && RECORD_PREFIXES.contains(&prefix));
            if known.is_none() && prefix != "HEX" && !is_record {
                continue;
            }
            // A chat command spelled out for viewers (`!hint HEX-05 ...`) takes the id itself.
            if in_command(&text[..m.start()]) {
                continue;
            }
            n += 1;
            let before = &text[at..m.start()];
            let after = &text[m.end()..];
            // `(HEX-13)` or `[HEX-13]` beside a name: the aside goes.
            let enclosed = (before.ends_with('(') && after.starts_with(')'))
                || (before.ends_with('[') && after.starts_with(']'));
            if enclosed {
                out.push_str(before[..before.len() - 1].trim_end_matches(' '));
                at = m.end() + 1;
                continue;
            }
            // `HEX-11 and lunarstake-help.example`: the name is said next; the id goes.
            if let Some(name) = known {
                if let Some(skip) = names_next(after, name.spoken()) {
                    out.push_str(before);
                    at = m.end() + skip;
                    let rest = &text[at..];
                    if sentence_start(&out) {
                        if let Some(c) = rest.chars().next() {
                            out.extend(c.to_uppercase());
                            at += c.len_utf8();
                        }
                    }
                    continue;
                }
            }
            out.push_str(before);
            let mut said = match known {
                Some(name) if is_record && !determined(&out) => {
                    let s = name.spoken();
                    if s.to_lowercase().starts_with("the ") {
                        s.to_string()
                    } else {
                        format!("the {s}")
                    }
                }
                Some(name) => name.spoken().to_string(),
                None if is_record => THAT_RECORD.to_string(),
                None => THAT_CARD.to_string(),
            };
            if sentence_start(&out) {
                said = capitalise(&said);
            }
            out.push_str(&said);
            at = m.end();
        }
        out.push_str(&text[at..]);
        out.push_str(tail);
        (out, n)
    }
}

/// How many bytes of `after` to skip when it goes straight on to `name` (`" and the Name"`,
/// `", Name"`): the joiner, so the name stays.
fn names_next(after: &str, name: &str) -> Option<usize> {
    let first = name.split_whitespace().next()?.to_lowercase();
    for joiner in [" and ", ", ", " "] {
        let Some(rest) = after.strip_prefix(joiner) else {
            continue;
        };
        let word = rest.strip_prefix("the ").unwrap_or(rest);
        if word.to_lowercase().starts_with(&first) {
            return Some(joiner.len());
        }
    }
    None
}

/// Whether the text before an id ends with a chat command word (`!hint `, `!grade `).
fn in_command(before: &str) -> bool {
    let Some(rest) = before.strip_suffix(' ') else {
        return false;
    };
    let word = rest
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or_default()
        .trim_start_matches(['"', '\'', '`', '(']);
    word.len() > 1 && word.starts_with('!') && word[1..].chars().all(|c| c.is_ascii_lowercase())
}

/// Whether the text so far ends at a sentence start (nothing yet, or after `.`, `!`, `?`).
fn sentence_start(out: &str) -> bool {
    let t = out.trim_end();
    t.is_empty() || t.ends_with(['.', '!', '?'])
}

/// Whether the text so far ends with a word that already determines a noun ("the", "this").
fn determined(out: &str) -> bool {
    let last = out
        .trim_end()
        .rsplit(|c: char| c.is_whitespace())
        .next()
        .unwrap_or_default()
        .to_lowercase();
    matches!(
        last.as_str(),
        "the" | "a" | "an" | "this" | "that" | "its" | "their" | "our" | "my" | "your"
    ) || last.ends_with("'s")
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// One entry of the case-file line.
struct FileEntry<'a> {
    hex: &'a str,
    record: &'a str,
    grade: &'a str,
    title: &'a str,
}

/// The entries of the runtime's case-file line (`case file (<kind>): HEX REC GRADE: title; ...`),
/// or `None` for any other line.
fn case_file_entries(line: &str) -> Option<Vec<FileEntry<'_>>> {
    let (_, rest) = line.trim().split_once("case file (")?;
    let (_, list) = rest.split_once("): ")?;
    let entries = list
        .split("; ")
        .filter_map(|e| {
            let (head, title) = e.split_once(": ")?;
            let mut w = head.split_whitespace();
            let (hex, record, grade) = (w.next()?, w.next()?, w.next()?);
            Some(FileEntry {
                hex,
                record,
                grade,
                title,
            })
        })
        .collect();
    Some(entries)
}

/// The lines a turn adds after the segment's rules when the context carries case cards: the
/// names rule, and in SEG-02A the segment's name. `None` without cards.
pub fn rules_lines(ctx: &SegmentContext) -> Option<String> {
    if ctx.hexes.is_empty() {
        return None;
    }
    let mut s = NAMES_RULE.to_string();
    if ctx.segment == SegmentId::Seg02aHexmap {
        s.push(' ');
        s.push_str(INVESTIGATION_LINE);
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::HexKind;

    fn card(id: &str, label: &str, fog: bool) -> Hex {
        Hex {
            id: id.into(),
            q: 0,
            r: 0,
            kind: HexKind::Entity,
            label: label.into(),
            grade: None,
            fog,
            sources: Vec::new(),
            role: None,
            date: None,
        }
    }

    /// The demo case's cards and records the show-3 lines name (fictional).
    fn show3() -> SegmentContext {
        SegmentContext {
            segment: SegmentId::Seg02aHexmap,
            rules_prompt: String::new(),
            state_summary: "case file (planted FICTIONAL demo records): HEX-01 DOC-01 DOC: \
                            Morrowgate Financial Conduct Office order 0001 (fictional); HEX-13 \
                            DOC-04 DOC: Morrowgate Financial Conduct Office warning list, 2026 \
                            (fictional)"
                .into(),
            state: "investigating".into(),
            allowed_ops: vec!["reveal_hex".into()],
            ccv: 30,
            elapsed_s: 60,
            hexes: vec![
                card("HEX-01", "Glimmerquay Yield Syndicate (fictional)", false),
                card("HEX-02", "glimmerquay-yield.example", false),
                card("HEX-11", "lunarstake-help.example", true),
                card(
                    "HEX-13",
                    "Morrowgate Financial Conduct Office, regulator (fictional)",
                    false,
                ),
            ],
        }
    }

    #[test]
    fn the_three_show_3_lines_are_spoken_in_names() {
        let n = CaseNames::read(&show3());
        let cases = [
            (
                "That's the demo's record, fictional, and it pins the regulator to HEX-02.",
                "That's the demo's record, fictional, and it pins the regulator to \
                 glimmerquay-yield.example.",
                1,
            ),
            (
                "DOC-01 is the demo's order: the Morrowgate regulator, fictional, orders the \
                 Glimmerquay Yield Syndicate.",
                "The Morrowgate Financial Conduct Office order 0001 is the demo's order: the \
                 Morrowgate regulator, fictional, orders the Glimmerquay Yield Syndicate.",
                1,
            ),
            (
                "Bone-clerks, to HEX-11 and lunarstake-help.example, which the regulator's \
                 warning list names.",
                "Bone-clerks, to lunarstake-help.example, which the regulator's warning list \
                 names.",
                1,
            ),
            (
                "Bone-clerks, to HEX-13 and the Morrowgate regulator, fictional as the rest.",
                "Bone-clerks, to the Morrowgate regulator, fictional as the rest.",
                1,
            ),
            (
                "HEX-11 is back, and DOC-04 names it.",
                "Lunarstake-help.example is back, and the Morrowgate Financial Conduct Office \
                 warning list names it.",
                2,
            ),
        ];
        for (line, want, k) in cases {
            let (got, count) = n.speak(line);
            assert_eq!(got, want);
            assert_eq!(count, k, "{line}");
            assert!(!id_regex().is_match(&got), "{got}");
        }
    }

    #[test]
    fn unknown_ids_become_that_card_or_record_and_other_tokens_and_ops_stay() {
        let n = CaseNames::read(&show3());
        assert_eq!(
            n.speak("HEX-99 is fogged, and REP-09 says nothing.").0,
            "That card is fogged, and that record says nothing."
        );
        assert_eq!(
            n.speak("The regulator (HEX-13) moved.").0,
            "The regulator moved."
        );
        // Not a card or a record: left alone; nothing counted.
        assert_eq!(n.speak("Back to SEG-01 and COVID-19.").1, 0);
        // A chat command spelled out for viewers takes the id (show-3: "!hint HEX-05 ...").
        let hint = "Type !hint, then a hex id, then your lead: \"!hint HEX-02 something\".";
        assert_eq!(n.speak(hint), (hint.to_string(), 0));
        // An op is action, never speech.
        let op = r#"Flipping it. OP: {"op":"reveal_hex","hex":"HEX-13","grade":"DOC"}"#;
        assert_eq!(n.speak(op), (op.to_string(), 0));
        // No case at all: a stray card id is still never spoken.
        assert_eq!(
            CaseNames::default().speak("HEX-02 again.").0,
            "That card again."
        );
    }

    #[test]
    fn the_prompt_shows_names_before_ids() {
        let ctx = show3();
        let n = CaseNames::read(&ctx);
        let summary = "revealed 2/4: HEX-01 DOC: Glimmerquay Yield Syndicate (fictional); HEX-13 \
                       REP ALLEGED, jury DOC: Morrowgate Financial Conduct Office, regulator \
                       (fictional)\nfogged: HEX-02, HEX-11\n";
        let text = format!("{summary}{}\nScreen: browser on DOC-04", ctx.state_summary);
        let got = n.prompt_text(&text);
        assert_eq!(
            got,
            "revealed 2/4: Glimmerquay Yield Syndicate [HEX-01] DOC; Morrowgate Financial \
             Conduct Office, regulator [HEX-13] REP ALLEGED, jury DOC\n\
             fogged: glimmerquay-yield.example [HEX-02], lunarstake-help.example [HEX-11]\n\
             case file (planted FICTIONAL demo records): Morrowgate Financial Conduct Office \
             order 0001 [DOC-01] DOC, on Glimmerquay Yield Syndicate [HEX-01]; Morrowgate \
             Financial Conduct Office warning list, 2026 [DOC-04] DOC, on Morrowgate Financial \
             Conduct Office, regulator [HEX-13]\n\
             Screen: browser on Morrowgate Financial Conduct Office warning list, 2026 [DOC-04]"
        );
        // A research report's `HEX-11 (label)` is re-ordered, never named twice.
        assert_eq!(
            n.prompt_text("- r1 done: HEX-11 (lunarstake-help.example): 1 record on file."),
            "- r1 done: lunarstake-help.example [HEX-11]: 1 record on file."
        );
        // Without cards nothing changes.
        assert_eq!(CaseNames::default().prompt_text(&text), text);
    }
}
