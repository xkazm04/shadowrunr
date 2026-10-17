//! Small shared helpers: hashing, dates, text normalisation.

use std::collections::BTreeSet;

/// FNV-1a 64. Stable across processes and Rust releases (unlike `DefaultHasher`), so a
/// prefix hash recorded today still compares tomorrow.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// (year, month) of a Unix-epoch millisecond timestamp (proleptic Gregorian, UTC).
pub fn year_month(ts_ms: i64) -> (i64, u32) {
    let days = ts_ms.div_euclid(86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32)
}

/// Lowercase, keep alphanumerics, collapse everything else into single spaces.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                out.push(l);
            }
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

fn trigrams(norm: &str) -> BTreeSet<[char; 3]> {
    let chars: Vec<char> = norm.chars().collect();
    let mut set = BTreeSet::new();
    if chars.len() < 3 {
        if !chars.is_empty() {
            let mut g = [' '; 3];
            for (i, c) in chars.iter().enumerate() {
                g[i] = *c;
            }
            set.insert(g);
        }
        return set;
    }
    for w in chars.windows(3) {
        set.insert([w[0], w[1], w[2]]);
    }
    set
}

/// Jaccard similarity of character trigrams of two normalised strings, 0.0..=1.0.
pub fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let ta = trigrams(a);
    let tb = trigrams(b);
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    inter / union
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "you", "your", "are", "was", "but", "not", "with", "this", "that", "have",
    "has", "had", "what", "who", "how", "why", "when", "where", "can", "will", "just", "they",
    "them", "his", "her", "its", "our", "out", "all", "any", "get", "got", "lol", "lmao", "from",
    "into", "about", "there", "their", "then", "than", "too", "very", "yes", "yeah",
];

/// Keyword tokens for an FTS5 MATCH: alphanumeric runs of 3+ chars, no stopwords, deduped,
/// at most `max`. Each token is quoted so FTS operators in chat text stay inert.
pub fn fts_query(text: &str, max: usize) -> Option<String> {
    let norm = normalize(text);
    let mut seen = BTreeSet::new();
    let mut toks = Vec::new();
    for t in norm.split(' ') {
        if t.chars().count() < 3 || STOPWORDS.contains(&t) {
            continue;
        }
        if seen.insert(t.to_string()) {
            toks.push(format!("\"{}\"", t.replace('"', "")));
        }
        if toks.len() >= max {
            break;
        }
    }
    if toks.is_empty() {
        None
    } else {
        Some(toks.join(" OR "))
    }
}

/// One line, at most `max` chars, with an ellipsis when cut.
pub fn one_line(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= max {
        flat.to_string()
    } else {
        let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(year_month(0), (1970, 1));
        // 2026-10-18T00:00:00Z
        assert_eq!(year_month(1_792_281_600_000), (2026, 10));
        assert_eq!(year_month(951_782_400_000), (2000, 2)); // 2000-02-29
    }

    #[test]
    fn normalise_and_similarity() {
        assert_eq!(normalize("  Hello,   WORLD!! "), "hello world");
        assert!(similarity("all hail the lich king", "all hail the lich kingg") > 0.8);
        assert!(similarity("what is your plan", "i love pizza") < 0.2);
    }

    #[test]
    fn fts_tokens_are_quoted() {
        let q = fts_query("NEAR(rubber) chicken OR \"x\"", 8).unwrap();
        assert_eq!(q, "\"near\" OR \"rubber\" OR \"chicken\"");
        assert!(fts_query("a an to", 8).is_none());
    }
}
