//! The core thread's output grammar.
//!
//! ```text
//! <plain text>          a spoken line (one Say per line)
//! OP: {json}            an action matching the contract's `Op`, allowed only if its tag is
//!                       in the segment's `allowed_ops`
//! NOTE: <candidate>     a memory candidate; becomes an episode, never a fact directly
//! FOIL <id>: <text>     a line for the active foil
//! ```

use serde_json::Value;
use shadowrunr_types::Op;

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Say(String),
    Foil {
        foil: String,
        text: String,
    },
    Op {
        tag: String,
        op: OpBox,
    },
    Note(String),
    /// An `OP:` line that failed: `rule` is `op_parse` or `op_not_allowed:<tag>`.
    Rejected {
        rule: String,
        raw: String,
    },
}

/// `Op` does not implement `PartialEq`; compare by JSON.
#[derive(Debug, Clone)]
pub struct OpBox(pub Op);

impl PartialEq for OpBox {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(&self.0).ok() == serde_json::to_value(&other.0).ok()
    }
}

/// The serde tag of an op (`"show_card"`...).
pub fn op_tag(op: &Op) -> String {
    serde_json::to_value(op)
        .ok()
        .and_then(|v| v.get("op").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

fn parse_json_lenient(s: &str) -> Option<Value> {
    let s = s.trim().trim_end_matches(',');
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        return Some(v);
    }
    // Repair one to three missing closing braces, as Athena's dispatcher does.
    let mut t = s.to_string();
    for _ in 0..3 {
        t.push('}');
        if let Ok(v) = serde_json::from_str::<Value>(&t) {
            return Some(v);
        }
    }
    None
}

fn strip_speaker(line: &str) -> &str {
    let l = line.trim();
    for p in ["**Malakor:**", "Malakor:", "MALAKOR:", "**Malakor**:"] {
        if let Some(rest) = l.strip_prefix(p) {
            return rest.trim();
        }
    }
    l
}

fn parse_op(json: &str, allowed: &[String]) -> Line {
    let raw = json.trim().to_string();
    let Some(v) = parse_json_lenient(&raw) else {
        return Line::Rejected {
            rule: "op_parse".into(),
            raw,
        };
    };
    let tag = v
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if tag.is_empty() {
        return Line::Rejected {
            rule: "op_parse".into(),
            raw,
        };
    }
    if !allowed.iter().any(|a| a == &tag) {
        return Line::Rejected {
            rule: format!("op_not_allowed:{tag}"),
            raw,
        };
    }
    match serde_json::from_value::<Op>(v) {
        Ok(op) => Line::Op { tag, op: OpBox(op) },
        Err(_) => Line::Rejected {
            rule: format!("op_parse:{tag}"),
            raw,
        },
    }
}

/// Parse the model's reply into ordered lines. `active_foil` gates `FOIL` lines.
pub fn parse_reply(text: &str, allowed_ops: &[String], active_foil: Option<&str>) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("OP:") {
            out.push(parse_op(rest, allowed_ops));
            continue;
        }
        if line.starts_with("{\"op\"") || line.starts_with("{ \"op\"") {
            out.push(parse_op(line, allowed_ops));
            continue;
        }
        // A NOTE anywhere on the line (models sometimes prefix a header such as
        // "Memory candidates (not spoken): NOTE: ...") is a memory candidate, never speech.
        if let Some(idx) = line.find("NOTE:") {
            let n = line[idx + "NOTE:".len()..].trim();
            if !n.is_empty() {
                out.push(Line::Note(n.to_string()));
            }
            continue;
        }
        // Meta lines about the output format are never spoken.
        if line.contains("(not spoken)") || line.eq_ignore_ascii_case("memory candidates:") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("FOIL ") {
            if let Some((id, text)) = rest.split_once(':') {
                let id = id.trim();
                let text = text.trim();
                if Some(id) == active_foil && !text.is_empty() {
                    out.push(Line::Foil {
                        foil: id.to_string(),
                        text: text.to_string(),
                    });
                } else {
                    out.push(Line::Rejected {
                        rule: format!("foil_not_active:{id}"),
                        raw: line.to_string(),
                    });
                }
                continue;
            }
        }
        let said = strip_speaker(line);
        if !said.is_empty() {
            out.push(Line::Say(said.to_string()));
        }
    }
    out
}

/// One JSON example per op tag, for the output-format instruction.
pub fn op_example(tag: &str) -> Option<&'static str> {
    Some(match tag {
        "highlight" => r#"{"op":"highlight","msg_id":"<msg id>"}"#,
        "start_poll" => {
            r#"{"op":"start_poll","poll_id":"p1","question":"...","options":["a","b"],"min_rank":"thrall","secs":60}"#
        }
        "close_poll" => r#"{"op":"close_poll","poll_id":"p1"}"#,
        "summon_foil" => r#"{"op":"summon_foil","foil":"<foil id>"}"#,
        "dismiss_foil" => r#"{"op":"dismiss_foil"}"#,
        "reveal_hex" => r#"{"op":"reveal_hex","hex":"HEX-01","grade":"REP"}"#,
        "dispatch_research" => {
            r#"{"op":"dispatch_research","job_id":"r1","brief":"what to find out"}"#
        }
        "show_card" => r#"{"op":"show_card","card_id":"c1","title":"...","body":"..."}"#,
        "mark_moment" => r#"{"op":"mark_moment","label":"..."}"#,
        "switch_segment" => r#"{"op":"switch_segment","to":"SEG-02A"}"#,
        "sign_off" => r#"{"op":"sign_off","reason":"..."}"#,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_lines_ops_and_notes() {
        let reply = "Bow, minions.\nOP: {\"op\":\"mark_moment\",\"label\":\"bow\"}\nNOTE: v1 loves chickens\nMalakor: Silence!";
        let lines = parse_reply(reply, &allowed(&["mark_moment"]), None);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], Line::Say("Bow, minions.".into()));
        assert!(matches!(&lines[1], Line::Op { tag, .. } if tag == "mark_moment"));
        assert_eq!(lines[2], Line::Note("v1 loves chickens".into()));
        assert_eq!(lines[3], Line::Say("Silence!".into()));
    }

    #[test]
    fn disallowed_op_is_rejected() {
        let reply = "OP: {\"op\":\"sign_off\",\"reason\":\"bored\"}";
        let lines = parse_reply(reply, &allowed(&["mark_moment"]), None);
        assert_eq!(
            lines,
            vec![Line::Rejected {
                rule: "op_not_allowed:sign_off".into(),
                raw: "{\"op\":\"sign_off\",\"reason\":\"bored\"}".into()
            }]
        );
    }

    #[test]
    fn repairs_missing_brace_and_rejects_garbage() {
        let lines = parse_reply(
            "OP: {\"op\":\"mark_moment\",\"label\":\"x\"\nOP: not json",
            &allowed(&["mark_moment"]),
            None,
        );
        assert!(matches!(&lines[0], Line::Op { .. }));
        assert!(matches!(&lines[1], Line::Rejected { rule, .. } if rule == "op_parse"));
    }

    #[test]
    fn every_op_example_parses() {
        for tag in [
            "highlight",
            "start_poll",
            "close_poll",
            "summon_foil",
            "dismiss_foil",
            "reveal_hex",
            "dispatch_research",
            "show_card",
            "mark_moment",
            "switch_segment",
            "sign_off",
        ] {
            let ex = op_example(tag).unwrap();
            let op: Op = serde_json::from_str(ex).unwrap_or_else(|e| panic!("{tag}: {e}"));
            assert_eq!(op_tag(&op), tag);
        }
    }

    #[test]
    fn note_with_a_header_is_never_spoken() {
        let reply = "Welcome, minion.
Memory candidates (not spoken): NOTE: v1 is new
Memory candidates:
Back to conquest.";
        let lines = parse_reply(reply, &[], None);
        let spoken: Vec<_> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Say(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(spoken, vec!["Welcome, minion.", "Back to conquest."]);
        assert!(lines
            .iter()
            .any(|l| matches!(l, Line::Note(n) if n == "v1 is new")));
    }
}
