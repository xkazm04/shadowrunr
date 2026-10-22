//! Chat command parser: `!summon`, `!hint`, `!grade`, `!propose`, `!vote`.

use shadowrunr_types::{Command, EvidenceGrade};

/// Free-text arguments (`!hint` text, `!propose` text, `!vote` option) are cut to this many
/// characters. An assumption: long enough for a premise, short enough to keep prompts small.
pub const MAX_TEXT_CHARS: usize = 280;

/// One chat command as [`parse_command`] reads it: the name, the argument shape and what it
/// does. The operator console shows this table (`segments.tables` `commands`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub args: &'static str,
    pub what: &'static str,
    /// Gated by the rank table's `propose_min_rank`.
    pub rank_gated: bool,
}

/// Every command [`parse_command`] accepts, in `table::COMMAND_NAMES` order.
pub const COMMAND_SPECS: [CommandSpec; 5] = [
    CommandSpec {
        name: "summon",
        args: "<foil>",
        what: "Call a guest foil into the room. The weighted vote decides.",
        rank_gated: false,
    },
    CommandSpec {
        name: "hint",
        args: "<hex> <text>",
        what: "Nominate a lead on the board.",
        rank_gated: false,
    },
    CommandSpec {
        name: "grade",
        args: "<hex> <DOC|REP|CLM|SPC>",
        what: "Grade a hex. Jury advice, never proof.",
        rank_gated: false,
    },
    CommandSpec {
        name: "propose",
        args: "<text>",
        what: "Propose a scheme. Rank-gated.",
        rank_gated: true,
    },
    CommandSpec {
        name: "vote",
        args: "<option>",
        what: "Vote in the open poll. Weight rises with rank.",
        rank_gated: false,
    },
];

/// Parse a chat line into a command. Command names are case-insensitive, whitespace is
/// tolerated, and arguments are validated: anything malformed yields `None`.
pub fn parse_command(text: &str) -> Option<Command> {
    let rest = text.trim().strip_prefix('!')?;
    // The command name must follow the `!` directly (`! summon` is chat, not a command).
    if rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut parts = rest.split_whitespace();
    let name = parts.next()?.to_ascii_lowercase();
    let args: Vec<&str> = parts.collect();
    match name.as_str() {
        "summon" => match args.as_slice() {
            [foil] => normalize_foil(foil).map(|foil| Command::Summon { foil }),
            _ => None,
        },
        "hint" => match args.as_slice() {
            [hex, text @ ..] if !text.is_empty() => Some(Command::Hint {
                hex: normalize_hex(hex)?,
                text: join_capped(text),
            }),
            _ => None,
        },
        "grade" => match args.as_slice() {
            [hex, grade] => Some(Command::Grade {
                hex: normalize_hex(hex)?,
                grade: parse_grade(grade)?,
            }),
            _ => None,
        },
        "propose" if !args.is_empty() => Some(Command::Propose {
            text: join_capped(&args),
        }),
        "vote" if !args.is_empty() => Some(Command::Vote {
            option: join_capped(&args),
        }),
        _ => None,
    }
}

/// `DOC|REP|CLM|SPC`, case-insensitive. Anything else (including the long forms) is `None`.
pub fn parse_grade(s: &str) -> Option<EvidenceGrade> {
    match s.to_ascii_uppercase().as_str() {
        "DOC" => Some(EvidenceGrade::Doc),
        "REP" => Some(EvidenceGrade::Rep),
        "CLM" => Some(EvidenceGrade::Clm),
        "SPC" => Some(EvidenceGrade::Spc),
        _ => None,
    }
}

/// Canonical hex id `HEX-NN`. Accepts `HEX-06`, `hex-6`, `hex6`, `Hex06` and a bare `6`, which
/// fixes the digest's `!hint hex6` vs `HEX-06` drift. Numbers run 1..=99.
pub fn normalize_hex(s: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let digits = lower
        .strip_prefix("hex")
        .map(|r| r.strip_prefix('-').unwrap_or(r))
        .unwrap_or(&lower);
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (1..=99).contains(&n).then(|| format!("HEX-{n:02}"))
}

/// Foil ids are lowercase slugs (`the-disruptor`).
fn normalize_foil(s: &str) -> Option<String> {
    let foil = s.to_ascii_lowercase();
    let ok = !foil.is_empty()
        && foil.len() <= 64
        && foil
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then_some(foil)
}

fn join_capped(words: &[&str]) -> String {
    words.join(" ").chars().take(MAX_TEXT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_specs_cover_the_command_names_and_parse() {
        let names: Vec<&str> = COMMAND_SPECS.iter().map(|c| c.name).collect();
        assert_eq!(names, crate::table::COMMAND_NAMES);
        let samples = [
            "!summon the-disruptor",
            "!hint HEX-06 the ledger",
            "!grade HEX-06 DOC",
            "!propose a scheme",
            "!vote two",
        ];
        for (spec, line) in COMMAND_SPECS.iter().zip(samples) {
            assert!(line.starts_with(&format!("!{}", spec.name)));
            assert!(parse_command(line).is_some(), "{line}");
        }
    }

    fn summon(s: &str) -> Option<String> {
        match parse_command(s)? {
            Command::Summon { foil } => Some(foil),
            _ => None,
        }
    }

    #[test]
    fn summon_variants() {
        assert_eq!(
            summon("!summon the-disruptor").as_deref(),
            Some("the-disruptor")
        );
        assert_eq!(
            summon("  !SUMMON   The-Disruptor  ").as_deref(),
            Some("the-disruptor")
        );
        assert_eq!(
            summon("!Summon\tthe_life_coach").as_deref(),
            Some("the_life_coach")
        );
        assert!(parse_command("!summon").is_none());
        assert!(parse_command("!summon the disruptor").is_none());
        assert!(parse_command("!summon <script>").is_none());
    }

    #[test]
    fn hint_variants() {
        match parse_command("!hint hex6   follow   the wallet ").unwrap() {
            Command::Hint { hex, text } => {
                assert_eq!(hex, "HEX-06");
                assert_eq!(text, "follow the wallet");
            }
            other => panic!("{other:?}"),
        }
        assert!(parse_command("!hint HEX-06").is_none(), "text is required");
        assert!(parse_command("!hint banana look here").is_none(), "bad hex");
        assert!(parse_command("!hint").is_none());
    }

    #[test]
    fn grade_variants() {
        match parse_command("!GRADE hex-03 doc").unwrap() {
            Command::Grade { hex, grade } => {
                assert_eq!(hex, "HEX-03");
                assert_eq!(grade, EvidenceGrade::Doc);
            }
            other => panic!("{other:?}"),
        }
        for (g, want) in [
            ("REP", EvidenceGrade::Rep),
            ("clm", EvidenceGrade::Clm),
            ("Spc", EvidenceGrade::Spc),
        ] {
            match parse_command(&format!("!grade HEX-01 {g}")).unwrap() {
                Command::Grade { grade, .. } => assert_eq!(grade, want),
                other => panic!("{other:?}"),
            }
        }
        assert!(
            parse_command("!grade HEX-01 TRUE").is_none(),
            "unknown grade"
        );
        assert!(
            parse_command("!grade HEX-01 DOCUMENTED_PRIMARY").is_none(),
            "long form is not a grade"
        );
        assert!(parse_command("!grade HEX-01").is_none());
        assert!(parse_command("!grade HEX-01 DOC extra").is_none());
        assert!(parse_command("!grade nope DOC").is_none());
    }

    #[test]
    fn propose_and_vote() {
        match parse_command("!propose  a duel   of riddles").unwrap() {
            Command::Propose { text } => assert_eq!(text, "a duel of riddles"),
            other => panic!("{other:?}"),
        }
        match parse_command("!VOTE  Option B").unwrap() {
            Command::Vote { option } => assert_eq!(option, "Option B"),
            other => panic!("{other:?}"),
        }
        assert!(parse_command("!propose").is_none());
        assert!(parse_command("!vote   ").is_none());
        let long = format!("!propose {}", "x".repeat(1000));
        match parse_command(&long).unwrap() {
            Command::Propose { text } => assert_eq!(text.chars().count(), MAX_TEXT_CHARS),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn malformed_input() {
        for s in [
            "",
            "   ",
            "!",
            "! summon x",
            "summon the-disruptor",
            "hello !summon the-disruptor",
            "!unknown thing",
            "!!summon x",
            "!summonx",
        ] {
            assert!(parse_command(s).is_none(), "{s:?} should not parse");
        }
    }

    #[test]
    fn hex_normalisation() {
        assert_eq!(normalize_hex("HEX-06").as_deref(), Some("HEX-06"));
        assert_eq!(normalize_hex("hex6").as_deref(), Some("HEX-06"));
        assert_eq!(normalize_hex("Hex-19").as_deref(), Some("HEX-19"));
        assert_eq!(normalize_hex("7").as_deref(), Some("HEX-07"));
        assert_eq!(normalize_hex("hex007").as_deref(), Some("HEX-07"));
        for bad in [
            "hex", "hex-", "HEX-0", "hex100", "hex-6a", "-6", "x6", "hex--6",
        ] {
            assert!(normalize_hex(bad).is_none(), "{bad}");
        }
    }
}
