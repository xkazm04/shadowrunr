//! What the segment is about right now, as words chat can share: the picker scores a message
//! that shares one (`topic`) and the orchestrator counts it as on-topic chat ([`crate::orchestrate`]).
//! Both read the one helper here, [`topic_words`], so they never disagree.
//!
//! The words come from the live segment's own state, never from chat: the case title and the
//! hex labels, the foils on offer (or on stage), the open poll's options and the current state's
//! purpose line. A word is a lowercase run of letters, four or more of them, that is not a
//! common word.

use shadowrunr_types::{Command, SegmentContext};

use crate::moderation::Candidate;
use crate::util::normalize;

/// Shortest topic word, in letters.
pub const TOPIC_MIN_LETTERS: usize = 4;

/// Common words of four letters or more: never a topic, whatever the segment says.
const STOPWORDS: &[&str] = &[
    "about",
    "after",
    "again",
    "against",
    "also",
    "always",
    "another",
    "anything",
    "around",
    "back",
    "been",
    "before",
    "being",
    "best",
    "both",
    "call",
    "came",
    "case",
    "cases",
    "chat",
    "come",
    "could",
    "does",
    "doing",
    "done",
    "down",
    "each",
    "even",
    "ever",
    "every",
    "first",
    "from",
    "game",
    "give",
    "good",
    "have",
    "here",
    "hexes",
    "into",
    "just",
    "know",
    "last",
    "least",
    "like",
    "live",
    "long",
    "look",
    "made",
    "make",
    "many",
    "more",
    "most",
    "much",
    "must",
    "never",
    "next",
    "night",
    "nothing",
    "once",
    "only",
    "open",
    "other",
    "over",
    "part",
    "poll",
    "really",
    "room",
    "said",
    "same",
    "says",
    "segment",
    "should",
    "show",
    "since",
    "some",
    "something",
    "state",
    "still",
    "such",
    "take",
    "tell",
    "than",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "thing",
    "things",
    "this",
    "those",
    "tonight",
    "told",
    "under",
    "until",
    "upon",
    "very",
    "vote",
    "want",
    "well",
    "were",
    "what",
    "when",
    "where",
    "which",
    "while",
    "will",
    "with",
    "would",
    "your",
    "yours",
];

/// Lines of the state summary the runtime writes itself (status, never the state's purpose).
const RUNTIME_PREFIXES: &[&str] = &["REFUSED", "HANDOVER", "CLOSING"];

fn content_words(text: &str, out: &mut Vec<String>) {
    for w in normalize(text).split(' ') {
        if w.chars().count() >= TOPIC_MIN_LETTERS
            && w.chars().all(|c| c.is_alphabetic())
            && !STOPWORDS.contains(&w)
            && !out.iter().any(|o| o == w)
        {
            out.push(w.to_string());
        }
    }
}

/// The current segment's topic words, in order of first appearance, each once:
/// - the case title (`Case: <title> (<id>), …` in the rules) and every hex label;
/// - the foils (`Foils you may summon: …` in the rules, `active foil: …` in the state);
/// - the open poll's options (`poll_options`);
/// - the current state's purpose: the state summary's line after its header, when the state
///   has one (it starts with a capital and is not a runtime status line).
pub fn topic_words(ctx: &SegmentContext, poll_options: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in ctx.rules_prompt.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Case: ") {
            let title = rest.rsplit_once(" (").map_or(rest, |(t, _)| t);
            content_words(title, &mut out);
        }
        if let Some(rest) = line.strip_prefix("Foils you may summon: ") {
            content_words(&rest.replace('-', " "), &mut out);
        }
    }
    for h in &ctx.hexes {
        content_words(&h.label, &mut out);
    }
    if let Some(purpose) = state_purpose(ctx) {
        content_words(&purpose, &mut out);
    }
    for line in ctx.state_summary.lines() {
        if let Some(f) = line.trim().strip_prefix("active foil: ") {
            content_words(&f.replace('-', " "), &mut out);
        }
    }
    for o in poll_options {
        content_words(o, &mut out);
    }
    out
}

/// The current state's purpose line, as [`topic_words`] finds it, when the state has one.
pub fn state_purpose(ctx: &SegmentContext) -> Option<String> {
    let mut lines = ctx.state_summary.lines();
    let header = lines.next().unwrap_or_default();
    if !header.starts_with("segment ") {
        return None;
    }
    lines
        .next()
        .map(str::trim)
        .filter(|l| {
            l.chars().next().is_some_and(char::is_uppercase)
                && !RUNTIME_PREFIXES.iter().any(|p| l.starts_with(p))
        })
        .map(str::to_string)
}

/// Whether `text` shares a topic word (a plural `s` either way still matches).
pub fn shares_topic(text: &str, words: &[String]) -> bool {
    if words.is_empty() {
        return false;
    }
    normalize(text).split(' ').any(|w| {
        w.chars().count() >= TOPIC_MIN_LETTERS
            && words.iter().any(|t| {
                t == w
                    || w.strip_suffix('s').is_some_and(|s| s == t)
                    || t.strip_suffix('s').is_some_and(|s| s == w)
            })
    })
}

/// Phrases that make a message an idea for the segment.
const IDEA_PHRASES: &[&str] = &[
    "what if",
    "you should",
    "we should",
    "let s",
    "lets",
    "idea",
    "how about",
];

/// Whether a message offers an idea for the segment: a `!propose`, or its normalised text holds
/// "what if", "you should", "we should", "let's", "idea" or "how about".
pub fn is_idea(c: &Candidate) -> bool {
    if matches!(c.msg.command, Some(Command::Propose { .. })) {
        return true;
    }
    let norm = format!(" {} ", normalize(&c.msg.text));
    IDEA_PHRASES.iter().any(|p| {
        if *p == "idea" {
            norm.split(' ').any(|w| w == "idea" || w == "ideas")
        } else {
            norm.contains(&format!(" {p} "))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shadowrunr_types::{ChatMsg, Hex, HexKind, Platform, Rank, SegmentId, Viewer};

    fn ctx(rules: &str, summary: &str, hexes: &[&str]) -> SegmentContext {
        SegmentContext {
            segment: SegmentId::Seg02aHexmap,
            rules_prompt: rules.into(),
            state_summary: summary.into(),
            state: "investigating".into(),
            allowed_ops: Vec::new(),
            ccv: 5,
            elapsed_s: 10,
            hexes: hexes
                .iter()
                .enumerate()
                .map(|(i, l)| Hex {
                    id: format!("HEX-{i:02}"),
                    q: 0,
                    r: 0,
                    kind: HexKind::Entity,
                    label: l.to_string(),
                    grade: None,
                    fog: true,
                    sources: Vec::new(),
                    role: None,
                    date: None,
                })
                .collect(),
        }
    }

    pub(crate) fn cand(text: &str, command: Option<Command>) -> Candidate {
        Candidate {
            msg: ChatMsg {
                id: "m".into(),
                ts_ms: 0,
                viewer: Viewer {
                    id: "v".into(),
                    name: "v".into(),
                    platform: Platform::Sim,
                    rank: Rank::Thrall,
                    sub_tier: None,
                    months: 0,
                    gifted: 0,
                    is_mod: false,
                },
                text: text.into(),
                command,
            },
            count: 1,
            viewers: 1,
            injection: false,
            safety: None,
        }
    }

    #[test]
    fn topic_words_come_from_the_case_the_foils_the_poll_and_the_purpose() {
        let c = ctx(
            "SEGMENT SEG-02A.\nCase: The Lantern Guild Swindle (case-7), 3 hexes. THIS CASE IS \
             FICTIONAL.\nFoils you may summon: the-inspector, the-coin-prophet.",
            "segment Hexmap (Investigation), state investigating\nTrace the treasury wallet before \
             the vote.\nactive foil: the-life-coach\nno poll open",
            &["Treasury Wallet", "Ledger 9"],
        );
        let w = topic_words(&c, &["moat".into(), "swampland".into()]);
        assert_eq!(
            w,
            [
                "lantern",
                "guild",
                "swindle",
                "inspector",
                "coin",
                "prophet",
                "treasury",
                "wallet",
                "ledger",
                "trace",
                "life",
                "coach",
                "moat",
                "swampland"
            ],
            "4+ letters, no common words, each once"
        );
        // A state with no purpose line: the runtime's status lines are no topic.
        let c = ctx(
            "R",
            "segment Improvised Room, state opening\nno poll open",
            &[],
        );
        assert!(topic_words(&c, &[]).is_empty());
        assert_eq!(state_purpose(&c), None);
        let c = ctx(
            "R",
            "segment Improvised Room, state cold_open\nThe room is still filling: tell the story.",
            &[],
        );
        assert_eq!(
            state_purpose(&c).as_deref(),
            Some("The room is still filling: tell the story.")
        );
    }

    #[test]
    fn a_message_shares_a_topic_or_offers_an_idea() {
        let words: Vec<String> = ["wallet".into(), "ledgers".into()].to_vec();
        assert!(shares_topic("who owns the WALLETS?", &words));
        assert!(shares_topic("check the ledger", &words));
        assert!(!shares_topic("nice hat", &words));
        assert!(!shares_topic("wallet", &[]));
        for t in [
            "what if the lich annexes the swamp",
            "you should open a poll",
            "Let's sue the ghosts",
            "lets go moat",
            "I have an idea",
            "how about a duel",
            "we should vote",
        ] {
            assert!(is_idea(&cand(t, None)), "{t}");
        }
        assert!(is_idea(&cand(
            "!propose a moat party",
            Some(Command::Propose {
                text: "a moat party".into()
            })
        )));
        for t in ["ideally not", "whatif", "nice hat", "you shoulder it"] {
            assert!(!is_idea(&cand(t, None)), "{t}");
        }
    }
}
