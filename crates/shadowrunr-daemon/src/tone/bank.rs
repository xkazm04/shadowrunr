//! `lines/<tone>.toml` edited as text: a new line is appended as its own `[[line]]` block and a
//! status is changed on its one `status = ...` line, so the owner's comments, ordering and
//! layout survive (repo law for hand-formatted files). Every edit is checked by parsing the
//! file before and after: the result must be the old bank plus exactly the intended change,
//! else nothing is written.

use std::collections::BTreeMap;

use shadowrunr_brain::character::{BankLine, LinesFile};
use shadowrunr_types::tone::{LineSituation, LineStatus, ToneLine};

/// The longest bank line the Tone Lab writes, in characters.
pub const MAX_LINE_CHARS: usize = 400;

/// Parse a bank file as written (every status, placeholders unfilled).
pub fn parse(text: &str) -> Result<LinesFile, String> {
    toml::from_str(text).map_err(|e| e.message().trim().to_string())
}

/// A bank line on the wire. A tag the contract does not know is left out (validation refuses
/// such a file anyway).
pub fn wire(l: &BankLine) -> ToneLine {
    ToneLine {
        id: l.id.clone(),
        status: l.status,
        tags: l
            .tags
            .iter()
            .filter_map(|t| LineSituation::parse(t))
            .collect(),
        text: l.text.clone(),
    }
}

/// One line of text, as spoken: trimmed, every run of whitespace a single space.
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A TOML basic string: `"..."` with `\` and `"` escaped, control characters as escapes.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn status_word(s: LineStatus) -> &'static str {
    match s {
        LineStatus::Seed => "seed",
        LineStatus::Approved => "approved",
        LineStatus::Retired => "retired",
    }
}

/// The id prefix a tone's lines take when its file has none to copy: the initials of its first
/// two words (`unhinged-roastmaster` -> `ur`), or the first two letters of a one-word tone
/// (`theatrical` -> `th`).
pub fn tone_prefix(tone: &str) -> String {
    let words: Vec<&str> = tone.split(['-', '_']).filter(|w| !w.is_empty()).collect();
    let p: String = match words.as_slice() {
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
        [a] => a.chars().take(2).collect(),
        [] => String::new(),
    };
    let p: String = p
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if p.is_empty() {
        "ln".into()
    } else {
        p
    }
}

/// The next id of the file's scheme, `<prefix>-<NN>`: the prefix most of its ids carry (else
/// [`tone_prefix`]), one past the highest number under it, zero-padded to the widest number
/// already used (at least two digits).
pub fn next_id(tone: &str, ids: &[String]) -> String {
    let mut by_prefix: BTreeMap<&str, (usize, u64, usize)> = BTreeMap::new();
    for id in ids {
        if let Some((p, n)) = id.rsplit_once('-') {
            if !p.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
                let e = by_prefix.entry(p).or_insert((0, 0, 0));
                e.0 += 1;
                e.1 = e.1.max(n.parse().unwrap_or(0));
                e.2 = e.2.max(n.len());
            }
        }
    }
    let best = by_prefix
        .iter()
        .max_by(|a, b| a.1 .0.cmp(&b.1 .0).then_with(|| b.0.cmp(a.0)));
    let (prefix, mut n, width) = match best {
        Some((p, (_, max, w))) => (p.to_string(), max + 1, (*w).max(2)),
        None => (tone_prefix(tone), 1, 2),
    };
    loop {
        let id = format!("{prefix}-{n:0width$}");
        if !ids.contains(&id) {
            return id;
        }
        n += 1;
    }
}

/// A new bank file's text: a header comment, `version` and `tone`.
pub fn new_file(tone: &str) -> String {
    format!(
        "# The {tone} line bank, started by the Tone Lab. Layout: docs/spec/contract.md,\n\
         # character package.\nversion = 1\ntone = {}\n",
        quote(tone)
    )
}

/// `text` with LF line ends, and whether it had CRLF ones (an edit keeps the file's style).
fn lf(text: &str) -> (String, bool) {
    (text.replace("\r\n", "\n"), text.contains("\r\n"))
}

fn restore_ends(text: String, crlf: bool) -> String {
    if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

/// Append `line` to the bank `text` as a new block after one blank line. Checked: the result
/// parses to the old bank plus that line.
pub fn append(text: &str, line: &BankLine) -> Result<String, String> {
    let before = parse(text)?;
    let (mut s, crlf) = lf(text);
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    if !s.is_empty() && !s.ends_with("\n\n") {
        s.push('\n');
    }
    let tags: Vec<String> = line.tags.iter().map(|t| quote(t)).collect();
    s.push_str(&format!(
        "[[line]]\nid = {}\nstatus = {}\ntags = [{}]\ntext = {}\n",
        quote(&line.id),
        quote(status_word(line.status)),
        tags.join(", "),
        quote(&line.text)
    ));
    let after = parse(&s)?;
    let mut expected = before.clone();
    expected.lines.push(line.clone());
    if after != expected {
        return Err("the appended block did not parse as the new line alone".into());
    }
    Ok(restore_ends(s, crlf))
}

/// Is `l` (trimmed) a `key = ...` line?
fn is_key(l: &str, key: &str) -> bool {
    l.trim_start()
        .strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// Set the status of the line `id` in `text` by editing its `status = ...` line (or inserting
/// one after its id). Checked: the result parses to the old bank with only that status changed.
pub fn set_status(text: &str, id: &str, status: LineStatus) -> Result<String, String> {
    let before = parse(text)?;
    let index = before
        .lines
        .iter()
        .position(|l| l.id == id)
        .ok_or_else(|| format!("no line {id:?}"))?;
    let (s, crlf) = lf(text);
    let mut rows: Vec<String> = s.split('\n').map(String::from).collect();
    let header = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.trim() == "[[line]]")
        .nth(index)
        .map(|(i, _)| i)
        .ok_or("the file's [[line]] headers do not match its lines")?;
    let end = rows
        .iter()
        .enumerate()
        .skip(header + 1)
        .find(|(_, r)| r.trim_start().starts_with('['))
        .map_or(rows.len(), |(i, _)| i);
    let new_row = format!("status = {}", quote(status_word(status)));
    match (header + 1..end).find(|&i| is_key(&rows[i], "status")) {
        Some(i) => {
            let indent: String = rows[i].chars().take_while(|c| c.is_whitespace()).collect();
            rows[i] = format!("{indent}{new_row}");
        }
        None => {
            let at = (header + 1..end)
                .find(|&i| is_key(&rows[i], "id"))
                .map_or(header + 1, |i| i + 1);
            rows.insert(at, new_row);
        }
    }
    let out = rows.join("\n");
    let after = parse(&out)?;
    let mut expected = before.clone();
    expected.lines[index].status = status;
    if after != expected {
        return Err(format!("the edit of {id:?} changed more than its status"));
    }
    Ok(restore_ends(out, crlf))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANK: &str = "# A comment the owner wrote.\nversion = 1\ntone = \"calm\"\n\n\
        [[line]]\nid = \"ca-01\"\nstatus = \"seed\"\ntags = [\"quiet\"]\ntext = \"One.\"\n\n\
        [[line]]\nid = \"ca-02\"\ntags = [\"bait\"] # no status: a seed\ntext = \"Two.\"\n";

    fn line(id: &str, text: &str) -> BankLine {
        BankLine {
            id: id.into(),
            status: LineStatus::Approved,
            tags: vec!["roast_request".into(), "general".into()],
            text: text.into(),
        }
    }

    #[test]
    fn ids_follow_the_file_scheme() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            next_id("unhinged-roastmaster", &ids(&["ur-01", "ur-28"])),
            "ur-29"
        );
        assert_eq!(next_id("calm", &ids(&["ca-001", "ca-009"])), "ca-010");
        assert_eq!(next_id("weary-middle-manager", &[]), "wm-01");
        assert_eq!(next_id("theatrical", &[]), "th-01");
        assert_eq!(
            next_id("x", &ids(&["odd", "zz-03", "ab-01", "ab-02"])),
            "ab-03"
        );
        assert_eq!(tone_prefix("--"), "ln");
    }

    #[test]
    fn quoting_round_trips_through_toml() {
        let nasty = "He said \"no\" \\ then\ttabbed \u{7} and 'quoted' {name}";
        let doc = format!("v = {}\n", quote(nasty));
        let v: toml::Table = toml::from_str(&doc).unwrap();
        assert_eq!(v["v"].as_str(), Some(nasty));
    }

    #[test]
    fn append_keeps_the_file_and_adds_one_block() {
        let out = append(BANK, &line("ca-03", "Three, \"quoted\".")).unwrap();
        assert!(out.starts_with(BANK), "{out}");
        assert!(out.ends_with(
            "\n\n[[line]]\nid = \"ca-03\"\nstatus = \"approved\"\ntags = [\"roast_request\", \"general\"]\ntext = \"Three, \\\"quoted\\\".\"\n"
        ), "{out}");
        let f = parse(&out).unwrap();
        assert_eq!(f.lines.len(), 3);
        // A file without a trailing newline, and a CRLF one, keep their style.
        let out = append(BANK.trim_end(), &line("ca-03", "x")).unwrap();
        assert!(out.contains("\"Two.\"\n\n[[line]]"), "{out}");
        let crlf = BANK.replace('\n', "\r\n");
        let out = append(&crlf, &line("ca-03", "x")).unwrap();
        assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
        assert!(out.starts_with(&crlf));
        let out = append(&new_file("calm"), &line("ca-01", "x")).unwrap();
        assert_eq!(parse(&out).unwrap().lines.len(), 1);
    }

    #[test]
    fn set_status_edits_one_line_or_inserts_it() {
        let out = set_status(BANK, "ca-01", LineStatus::Approved).unwrap();
        assert_eq!(
            out,
            BANK.replace("status = \"seed\"", "status = \"approved\"")
        );
        let out = set_status(BANK, "ca-02", LineStatus::Retired).unwrap();
        assert!(
            out.contains(
                "id = \"ca-02\"\nstatus = \"retired\"\ntags = [\"bait\"] # no status: a seed"
            ),
            "{out}"
        );
        let f = parse(&out).unwrap();
        assert_eq!(f.lines[1].status, LineStatus::Retired);
        assert_eq!(f.lines[0].status, LineStatus::Seed);
        assert!(set_status(BANK, "nope", LineStatus::Retired)
            .unwrap_err()
            .contains("no line"));
        // A header hidden in a multi-line string would misplace the edit: refused, not written.
        let tricky = "version = 1\ntone = \"calm\"\n\n[[line]]\nid = \"a\"\ntags = [\"quiet\"]\n\
            text = \"\"\"\n[[line]]\nstatus = \"seed\"\"\"\"\n\n[[line]]\nid = \"b\"\ntags = [\"quiet\"]\ntext = \"b\"\n";
        assert!(parse(tricky).is_ok());
        assert!(set_status(tricky, "b", LineStatus::Retired).is_err());
    }
}
