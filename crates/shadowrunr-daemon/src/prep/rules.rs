//! The stream-prep rules, held once. The planner's prompt text, its reply schema, the
//! mechanical checks and the guide's check table all render from this table, so what the
//! planner is told, what its reply is constrained to and what the approval gate enforces are
//! the same rule set (a test asserts each rendering, the guide's rows included).

use serde_json::{json, Value};
use shadowrunr_types::prep::{BitKind, ConnectorKind};

/// The longest line a prep may hold, in characters (lines are spoken aloud).
pub const MAX_LINE_CHARS: usize = 220;
/// Cold-open beats, fewest and most.
pub const COLD_OPEN_BEATS: (usize, usize) = (6, 10);
/// Connector lines of each kind, at least.
pub const MIN_CONNECTORS_PER_KIND: usize = 3;

/// Every connector kind, in wire order.
pub const CONNECTOR_KINDS: [ConnectorKind; 5] = [
    ConnectorKind::IntoRoom,
    ConnectorKind::IntoBit,
    ConnectorKind::BackToChat,
    ConnectorKind::IntoPoll,
    ConnectorKind::IntoHandover,
];

/// What each connector kind is for, in [`CONNECTOR_KINDS`] order (the prompt's text).
const CONNECTOR_PURPOSE: [&str; 5] = [
    "from the cold open into the room",
    "from chat back to a prepared bit",
    "from a bit back to chat",
    "into a vote",
    "toward the segment handover",
];

/// One kind of prepared bit: how many a prep holds, how many lines each, whether a payoff is
/// required, and how the planner is asked for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitRule {
    pub kind: BitKind,
    /// The reply's array for this kind, and the plural in prose.
    pub plural: &'static str,
    /// The kind's name in prose.
    pub singular: &'static str,
    /// Fewest and (when bounded) most bits of this kind.
    pub count: (usize, Option<usize>),
    /// Fewest and most lines per bit.
    pub lines: (usize, usize),
    pub payoff_required: bool,
    /// What the planner is asked to write, after the count.
    pub ask: &'static str,
    /// Said after the rule in the prompt.
    pub note: &'static str,
}

/// Every bit kind, in prompt and schema order.
pub const BITS: [BitRule; 3] = [
    BitRule {
        kind: BitKind::Premise,
        plural: "premises",
        singular: "premise",
        count: (12, None),
        lines: (1, 1),
        payoff_required: true,
        ask: "invitations chat can join in one line, standing alone, across the segments above",
        note: "",
    },
    BitRule {
        kind: BitKind::Story,
        plural: "stories",
        singular: "story",
        count: (3, Some(5)),
        lines: (2, 5),
        payoff_required: true,
        ask: "short told stories",
        note: " Required; not premises.",
    },
    BitRule {
        kind: BitKind::Callback,
        plural: "callbacks",
        singular: "callback",
        count: (3, Some(5)),
        lines: (1, 3),
        payoff_required: true,
        ask: "running gags from the bible, re-dressed for tonight, needing no earlier stream",
        note: " Required; not premises.",
    },
];

/// The rule for `kind`.
pub fn rule(kind: BitKind) -> &'static BitRule {
    BITS.iter()
        .find(|r| r.kind == kind)
        .expect("every bit kind has a rule")
}

fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// `a-b`, or `a` when both are the same.
fn range(min: usize, max: usize) -> String {
    if min == max {
        min.to_string()
    } else {
        format!("{min}-{max}")
    }
}

impl BitRule {
    /// The count as prose: `at least 12`, `3-5`.
    pub fn count_text(&self) -> String {
        match self.count.1 {
            Some(max) => range(self.count.0, max),
            None => format!("at least {}", self.count.0),
        }
    }

    /// The lines per bit as prose: `one`, `2-5`.
    pub fn lines_text(&self) -> String {
        if self.lines == (1, 1) {
            "one".into()
        } else {
            range(self.lines.0, self.lines.1)
        }
    }

    /// Whether `n` bits of this kind is within the count.
    pub fn count_ok(&self, n: usize) -> bool {
        n >= self.count.0 && self.count.1.is_none_or(|max| n <= max)
    }

    /// Whether `n` lines is within the lines per bit.
    pub fn lines_ok(&self, n: usize) -> bool {
        (self.lines.0..=self.lines.1).contains(&n)
    }

    /// The row's line in the planner's prompt.
    pub fn prompt_line(&self) -> String {
        let lines = if self.lines == (1, 1) {
            "one line each".to_string()
        } else {
            format!("{} lines each", self.lines_text())
        };
        let payoff = if self.payoff_required {
            ", with a payoff the host lands"
        } else {
            ""
        };
        format!(
            "- {}: {} {} ({lines}{payoff}).{}",
            self.plural,
            self.count_text(),
            self.ask,
            self.note
        )
    }

    /// The row's array in the reply schema.
    pub fn schema(&self, segments: &[String]) -> Value {
        let line = line_schema();
        let mut required = vec!["segment", "lines"];
        if self.payoff_required {
            required.push("payoff");
        }
        let mut arr = json!({
            "type": "array",
            "minItems": self.count.0,
            "items": {
                "type": "object",
                "required": required,
                "properties": {
                    "segment": {"type": "string", "enum": segments},
                    "lines": {"type": "array", "items": line, "minItems": self.lines.0, "maxItems": self.lines.1},
                    "payoff": line
                }
            }
        });
        if let Some(max) = self.count.1 {
            arr["maxItems"] = json!(max);
        }
        arr
    }
}

/// One spoken line in the reply schema.
pub fn line_schema() -> Value {
    json!({"type": "string", "maxLength": MAX_LINE_CHARS})
}

/// The connector kinds' wire names, in order.
pub fn connector_wire_names() -> Vec<String> {
    CONNECTOR_KINDS.iter().map(wire).collect()
}

/// The "What to write" section of the planner's prompt.
pub fn prompt_lines() -> String {
    let mut s = format!(
        "- cold_open: a title and {}-{} beat lines telling one short tale from the Spire in \
order, then a handoff line that turns from the story to the room.\n",
        COLD_OPEN_BEATS.0, COLD_OPEN_BEATS.1
    );
    for r in &BITS {
        s.push_str(&r.prompt_line());
        s.push('\n');
    }
    let kinds: Vec<String> = CONNECTOR_KINDS
        .iter()
        .zip(CONNECTOR_PURPOSE)
        .map(|(k, what)| format!("{} ({what})", wire(k)))
        .collect();
    s.push_str(&format!(
        "- connectors: at least {MIN_CONNECTORS_PER_KIND} lines of each kind: {}.\n\
- Every line at most {MAX_LINE_CHARS} characters; no line repeated.\n",
        kinds.join(", ")
    ));
    s
}

/// The guide's `counts` and `shape` rows (check id, "fails when" text), as
/// `docs/guide/cli.md` must print them.
pub fn doc_rows() -> Vec<(&'static str, String)> {
    let mut counts = vec![format!(
        "the cold open is not {}-{} beats",
        COLD_OPEN_BEATS.0, COLD_OPEN_BEATS.1
    )];
    for r in &BITS {
        counts.push(match r.count.1 {
            Some(max) => format!("{} outside {}", r.plural, range(r.count.0, max)),
            None => format!("fewer than {} {}", r.count.0, r.plural),
        });
    }
    counts.push(format!(
        "fewer than {MIN_CONNECTORS_PER_KIND} connectors of any kind"
    ));
    let mut shape: Vec<String> = BITS
        .iter()
        .map(|r| {
            let lines = if r.lines == (1, 1) {
                "one line".to_string()
            } else {
                format!("{} lines", r.lines_text())
            };
            format!("a {} is not {lines}", r.singular)
        })
        .collect();
    let paid: Vec<&str> = BITS
        .iter()
        .filter(|r| r.payoff_required)
        .map(|r| r.singular)
        .collect();
    if paid.len() == BITS.len() {
        shape.push("a bit has no payoff".into());
    } else if !paid.is_empty() {
        shape.push(format!("a {} has no payoff", paid.join(" or ")));
    }
    shape.push("a line is empty, or the reply had an item of unknown kind".into());
    vec![("counts", counts.join("; ")), ("shape", shape.join(", "))]
}
