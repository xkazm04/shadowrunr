//! Where a line's sentences end, for a renderer that speaks one sentence per request (the
//! ElevenLabs renderer): the sidecar's own rules (`tools/voice/shadowrunr_voice/text.py`), so a
//! line comes out in the same parts whichever provider speaks it.
//!
//! - A decimal, an abbreviation ("Dr.", "e.g.") or a stop before a lowercase word does not end
//!   a sentence, and closing quotes and brackets stay with the sentence they close.
//! - A fragment shorter than [`FOLD_UNDER`] joins the sentence before it.
//! - A first sentence longer than [`FIRST_MAX`] is cut at a comma, so the first audio starts
//!   early.

use std::sync::OnceLock;

use regex::Regex;

/// A first sentence longer than this is cut at a comma, when it has one in a usable place.
pub const FIRST_MAX: usize = 120;
/// A sentence shorter than this joins the one before it.
pub const FOLD_UNDER: usize = 12;

const CLOSERS: &[char] = &['"', '\'', ')', ']', '’', '”'];
const OPENERS: &[char] = &['(', '"', '\'', '‘', '“'];
const ABBREVIATIONS: [&str; 13] = [
    "dr", "mr", "mrs", "ms", "prof", "st", "jr", "sr", "vs", "e.g", "i.e", "cf", "approx",
];

fn boundary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"[.!?…]+["')\]’”]*\s+"#).expect("a valid pattern"))
}

fn ends_sentence(text: &str, start: usize, end: usize) -> bool {
    let stop = text[start..end].trim_end();
    if !stop.trim_end_matches(CLOSERS).ends_with('.') {
        return true;
    }
    if text[end..].chars().next().is_some_and(char::is_lowercase) {
        return false;
    }
    let word = text[..start].split_whitespace().next_back().unwrap_or("");
    let word = word.trim_start_matches(OPENERS).to_lowercase();
    word.is_empty() || !ABBREVIATIONS.contains(&word.as_str())
}

/// `text` cut where a sentence ends, blanks dropped; runs of whitespace become one space. A run
/// with no stop is one sentence.
pub fn sentences(text: &str) -> Vec<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = Vec::new();
    let mut start = 0;
    for m in boundary().find_iter(&text) {
        if ends_sentence(&text, m.start(), m.end()) {
            out.push(text[start..m.end()].trim().to_string());
            start = m.end();
        }
    }
    out.push(text[start..].trim().to_string());
    out.retain(|s| !s.is_empty());
    out
}

/// `sentence` whole, or cut in two at a comma when it is longer than `limit` characters: the
/// last comma inside the first `limit` that leaves at least [`FOLD_UNDER`] characters on each
/// side, else the first such comma anywhere. No usable comma leaves it whole.
pub fn first_cut(sentence: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = sentence.chars().collect();
    if chars.len() <= limit {
        return vec![sentence.to_string()];
    }
    let usable: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter(|&(at, c)| *c == ',' && at + 1 >= FOLD_UNDER && chars.len() - at > FOLD_UNDER)
        .map(|(at, _)| at)
        .collect();
    let Some(&first) = usable.first() else {
        return vec![sentence.to_string()];
    };
    let at = usable
        .iter()
        .copied()
        .rfind(|&a| a < limit)
        .unwrap_or(first);
    let head: String = chars[..=at].iter().collect();
    let tail: String = chars[at + 1..].iter().collect();
    vec![head.trim().to_string(), tail.trim().to_string()]
}

/// The line as it is rendered: sentences, short ones folded, a long first one cut.
pub fn split_line(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in sentences(text) {
        match out.last_mut() {
            Some(last) if part.chars().count() < FOLD_UNDER => {
                last.push(' ');
                last.push_str(&part);
            }
            _ => out.push(part),
        }
    }
    if let Some(first) = out.first().cloned() {
        out.splice(0..1, first_cut(&first, FIRST_MAX));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_end_at_stops_but_not_at_decimals_abbreviations_or_lowercase() {
        assert_eq!(
            sentences("Kneel. Now!  Why? Fine…  Done"),
            ["Kneel.", "Now!", "Why?", "Fine…", "Done"]
        );
        assert_eq!(
            sentences("It costs 3.5 souls. Pay."),
            ["It costs 3.5 souls.", "Pay."]
        );
        assert_eq!(
            sentences("Ask Dr. Bones. He knows."),
            ["Ask Dr. Bones.", "He knows."]
        );
        assert_eq!(sentences("So. then we wait."), ["So. then we wait."]);
        assert_eq!(
            sentences("He said \"Kneel.\" Then he left."),
            ["He said \"Kneel.\"", "Then he left."]
        );
        assert!(sentences("   ").is_empty());
    }

    #[test]
    fn short_fragments_fold_and_a_long_first_sentence_is_cut_at_a_comma() {
        assert_eq!(
            split_line("Minion. Come closer. No... closer."),
            ["Minion.", "Come closer.", "No... closer."]
        );
        assert_eq!(split_line("Kneel. Now. Go."), ["Kneel. Now. Go."]);
        assert_eq!(
            split_line("You brought me a tax form. To my crypt."),
            ["You brought me a tax form.", "To my crypt."]
        );
        let long = format!(
            "{}, and then the rest of the sentence goes on.",
            "a".repeat(110)
        );
        let parts = split_line(&long);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[0].ends_with(','));
        assert_eq!(parts[1], "and then the rest of the sentence goes on.");
        let no_comma = "b".repeat(130);
        assert_eq!(split_line(&no_comma), std::slice::from_ref(&no_comma));
    }
}
