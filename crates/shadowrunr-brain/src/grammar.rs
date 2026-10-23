//! The core thread's output grammar.
//!
//! ```text
//! <plain text>          a spoken line (one Say per line), optionally opened by a delivery
//!                       cue: [<emotion> <intensity> <pace> <gesture> eyes <n>] text
//! OP: {json}            an action matching the contract's `Op`, allowed only if its tag is
//!                       in the segment's `allowed_ops`
//! NOTE: <candidate>     a memory candidate; becomes an episode, never a fact directly
//! FOIL <id>: <text>     a line for the active foil
//! ```
//!
//! A spoken line's cue is parsed out of its text into a [`Delivery`] ([`spoken`]). Besides the
//! emotion words a cue may direct the face: one gesture ([`Gesture`]) and an eye light
//! (`eyes 2`), in any order after the emotion; a bracket of face words alone (`[glare]`,
//! `[eyes 0.4]`) is a neutral cue. Vendor
//! markup is not portable, so every other bracketed or asterisk span (`[laughs]`, `*sighs*`,
//! a cue naming an emotion outside the set) is stripped at the door: the voice never reads it
//! aloud.

use serde_json::Value;
use shadowrunr_types::delivery::{Delivery, Emotion, Gesture, Pace, DEFAULT_INTENSITY};
use shadowrunr_types::Op;

/// A spoken line: the text as said, and the delivery cue it opened with.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    pub text: String,
    pub delivery: Option<Delivery>,
}

impl Spoken {
    /// A line with no cue.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            delivery: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    Say(Spoken),
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

/// A line without the speaker label the model may start it with (`[identity]
/// reply_prefixes`).
fn strip_speaker<'a>(line: &'a str, prefixes: &[String]) -> &'a str {
    let l = line.trim();
    for p in prefixes {
        if let Some(rest) = l.strip_prefix(p.as_str()) {
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
pub fn parse_reply(
    text: &str,
    allowed_ops: &[String],
    active_foil: Option<&str>,
    prefixes: &[String],
) -> Vec<Line> {
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
        let said = spoken(strip_speaker(line, prefixes));
        if !said.text.is_empty() {
            out.push(Line::Say(said));
        }
    }
    out
}

/// The cue a line opens with (`[sarcasm 0.8 slow]`, or partial: `[glee]`, `[menace fast]`):
/// the emotion first, then an intensity and a pace in either order, each optional (intensity
/// [`DEFAULT_INTENSITY`], pace normal). After the emotion the cue may also name one gesture and
/// an eye light, `eyes <n>`, in any order (`[menace 0.9 slow glare eyes 2]`); the first gesture
/// wins. Words after the emotion that are none of these are ignored and the cue is kept
/// (`[menace 0.9 wink]` is menace 0.9). A bracket that opens with face words instead of an
/// emotion is a face-only cue (neutral, default intensity, normal pace) only when every word in
/// it is a face word (`[glare]`, `[eyes 0.4]`, `[nod eyes 1.5]`). `None` otherwise: the bracket
/// is markup and is stripped.
fn parse_cue(inner: &str) -> Option<Delivery> {
    let words: Vec<&str> = inner.split_whitespace().collect();
    let (emotion, rest) = match words.split_first() {
        Some((first, rest)) => match Emotion::parse(first) {
            Some(e) => (Some(e), rest),
            None => (None, words.as_slice()),
        },
        None => return None,
    };
    let mut intensity = DEFAULT_INTENSITY;
    let mut pace = Pace::Normal;
    let mut gesture: Option<Gesture> = None;
    let mut eye_light: Option<f32> = None;
    let mut face_only = true;
    let mut it = rest.iter().peekable();
    while let Some(w) = it.next() {
        if w.eq_ignore_ascii_case("eyes") {
            match it.peek().and_then(|n| n.parse::<f32>().ok()) {
                Some(n) => {
                    eye_light = Some(n);
                    it.next();
                }
                None => face_only = false,
            }
        } else if let Some(g) = Gesture::parse(w) {
            gesture = gesture.or(Some(g));
        } else {
            face_only = false;
            if let Ok(i) = w.parse::<f32>() {
                intensity = i;
            } else if let Some(p) = Pace::parse(w) {
                pace = p;
            }
        }
    }
    let emotion = match emotion {
        Some(e) => e,
        None if face_only && (gesture.is_some() || eye_light.is_some()) => Emotion::Neutral,
        None => return None,
    };
    Some(
        Delivery::new(emotion, intensity, pace)
            .with_gesture(gesture)
            .with_eye_light(eye_light),
    )
}

/// `text` without markup: every `[...]` span and `*...*` span (an action) goes, with any stray
/// `[`, `]` or `*`; a `**...**` emphasis keeps its words. Whitespace is collapsed, and none is
/// left before punctuation. A text with no markup comes back as it is.
pub fn strip_markup(text: &str) -> String {
    if !text.contains(['[', ']', '*']) {
        return text.to_string();
    }
    let unbolded = text.replace("**", "");
    let mut out = String::with_capacity(unbolded.len());
    let mut rest = unbolded.as_str();
    while let Some(i) = rest.find(['[', '*']) {
        out.push_str(&rest[..i]);
        let close = if rest[i..].starts_with('[') { ']' } else { '*' };
        rest = match rest[i + 1..].find(close) {
            Some(j) => &rest[i + 1 + j + 1..],
            None => &rest[i + 1..],
        };
        out.push(' ');
    }
    out.push_str(rest);
    let out = out.replace(']', " ");
    let mut tidy = String::with_capacity(out.len());
    for w in out.split_whitespace() {
        let glued = w.starts_with([',', '.', '!', '?', ';', ':', '\u{2026}']);
        if !tidy.is_empty() && !glued {
            tidy.push(' ');
        }
        tidy.push_str(w);
    }
    tidy
}

/// A spoken line with its opening cue parsed out and every other markup stripped.
pub fn spoken(line: &str) -> Spoken {
    let line = line.trim();
    let cue = line
        .strip_prefix('[')
        .and_then(|r| r.split_once(']'))
        .and_then(|(inner, rest)| parse_cue(inner).map(|d| (d, rest)));
    let (delivery, rest) = match cue {
        Some((d, rest)) => (Some(d), rest),
        None => (None, line),
    };
    Spoken {
        text: strip_markup(rest.trim()),
        delivery,
    }
}

/// The output record of a model line that makes more than [`MAX_EMOTION_RUN`] lines in a row
/// cued with one emotion. Recorded so a run can measure cue variety; never enforced.
pub const DELIVERY_REPEAT_RULE: &str = "delivery_repeat";
/// Its action.
pub const DELIVERY_REPEATED: &str = "repeated";
/// The most lines in a row the prompt asks to cue with one emotion.
pub const MAX_EMOTION_RUN: u32 = 2;

/// The run of the host's lines cued with one emotion. A line with no cue ends the run. A
/// neutral cue that directs the face (a gesture or an eye light, as `[glare]` parses) is a face
/// direction, not an emotion choice: it neither extends nor ends the run.
#[derive(Debug, Clone, Default)]
pub struct CueRun {
    last: Option<Emotion>,
    len: u32,
}

impl CueRun {
    /// Note the next spoken line's cue; true when it makes the run longer than
    /// [`MAX_EMOTION_RUN`].
    pub fn note(&mut self, delivery: Option<&Delivery>) -> bool {
        if delivery.is_some_and(|d| {
            d.emotion == Emotion::Neutral && (d.gesture.is_some() || d.eye_light.is_some())
        }) {
            return false;
        }
        let emotion = delivery.map(|d| d.emotion);
        if emotion.is_some() && emotion == self.last {
            self.len += 1;
        } else {
            self.last = emotion;
            self.len = u32::from(emotion.is_some());
        }
        self.len > MAX_EMOTION_RUN
    }
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
        "show_screen" => r#"{"op":"show_screen","screen":"browser","record_id":"DOC-01"}"#,
        "pin_link" => {
            r#"{"op":"pin_link","from":"HEX-04","to":"HEX-09","rel":"paid","record_id":"CLM-02"}"#
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes() -> Vec<String> {
        crate::tests::malakor_package().persona.reply_prefixes
    }
    use shadowrunr_types::{EvidenceGrade, LinkRel, Rank, ScreenId, SegmentId};

    fn allowed(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_lines_ops_and_notes() {
        let reply = "Bow, minions.\nOP: {\"op\":\"mark_moment\",\"label\":\"bow\"}\nNOTE: v1 loves chickens\nMalakor: Silence!";
        let lines = parse_reply(reply, &allowed(&["mark_moment"]), None, &prefixes());
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], Line::Say(Spoken::plain("Bow, minions.")));
        assert!(matches!(&lines[1], Line::Op { tag, .. } if tag == "mark_moment"));
        assert_eq!(lines[2], Line::Note("v1 loves chickens".into()));
        assert_eq!(lines[3], Line::Say(Spoken::plain("Silence!")));
    }

    fn said(reply: &str) -> Vec<Spoken> {
        parse_reply(reply, &[], None, &prefixes())
            .into_iter()
            .filter_map(|l| match l {
                Line::Say(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_full_cue_is_parsed_out_of_the_text() {
        assert_eq!(
            said("[sarcasm 0.8 slow] Oh, wonderful."),
            vec![Spoken {
                text: "Oh, wonderful.".into(),
                delivery: Some(Delivery::new(Emotion::Sarcasm, 0.8, Pace::Slow)),
            }]
        );
        // After a speaker label, any case, intensity clamped.
        assert_eq!(
            said("Malakor: [MENACE 3 fast] Kneel.")[0].delivery,
            Some(Delivery::new(Emotion::Menace, 1.0, Pace::Fast))
        );
    }

    #[test]
    fn a_partial_cue_takes_the_defaults() {
        assert_eq!(
            said("[glee] Ha!"),
            vec![Spoken {
                text: "Ha!".into(),
                delivery: Some(Delivery::new(
                    Emotion::Glee,
                    DEFAULT_INTENSITY,
                    Pace::Normal
                )),
            }]
        );
        assert_eq!(
            said("[hushed fast] Listen.")[0].delivery,
            Some(Delivery::new(
                Emotion::Hushed,
                DEFAULT_INTENSITY,
                Pace::Fast
            ))
        );
        assert_eq!(
            said("[weary 0.3] Again.")[0].delivery,
            Some(Delivery::new(Emotion::Weary, 0.3, Pace::Normal))
        );
    }

    #[test]
    fn a_cue_with_an_unknown_emotion_is_stripped() {
        assert_eq!(
            said("[joyful 0.9 fast] Behold."),
            vec![Spoken::plain("Behold.")]
        );
    }

    #[test]
    fn bracketed_and_asterisk_markup_is_stripped() {
        assert_eq!(
            said("[laughs] Fools, all of you [laughs]."),
            vec![Spoken::plain("Fools, all of you.")]
        );
        assert_eq!(
            said("*sighs* Very well. *adjusts crown*"),
            vec![Spoken::plain("Very well.")]
        );
        // A cue keeps its delivery; later markup still goes, emphasis keeps its words.
        assert_eq!(
            said("[triumph 0.9 slow] It is **done** [cackles]!"),
            vec![Spoken {
                text: "It is done!".into(),
                delivery: Some(Delivery::new(Emotion::Triumph, 0.9, Pace::Slow)),
            }]
        );
        // A line that was nothing but markup is not spoken.
        assert!(said("[laughs]\n*sighs*").is_empty());
    }

    #[test]
    fn a_third_line_in_a_row_with_one_emotion_is_counted() {
        let g = Delivery::new(Emotion::Glee, 0.5, Pace::Normal);
        let m = Delivery::new(Emotion::Menace, 0.5, Pace::Normal);
        let mut run = CueRun::default();
        let seen: Vec<bool> = [
            Some(&g),
            Some(&g),
            Some(&g),
            Some(&g),
            Some(&m),
            None,
            Some(&m),
        ]
        .into_iter()
        .map(|d| run.note(d))
        .collect();
        assert_eq!(seen, vec![false, false, true, true, false, false, false]);
    }

    #[test]
    fn a_cue_may_direct_the_face() {
        assert_eq!(
            said("[menace 0.9 slow glare eyes 2] Kneel."),
            vec![Spoken {
                text: "Kneel.".into(),
                delivery: Some(
                    Delivery::new(Emotion::Menace, 0.9, Pace::Slow)
                        .with_gesture(Some(Gesture::Glare))
                        .with_eye_light(Some(2.0))
                ),
            }]
        );
        // Order after the emotion is free; the first gesture wins; eyes are clamped.
        assert_eq!(
            said("[glee eyes 9 nod fast tilt 0.3] Ha!")[0].delivery,
            Some(
                Delivery::new(Emotion::Glee, 0.3, Pace::Fast)
                    .with_gesture(Some(Gesture::Nod))
                    .with_eye_light(Some(2.5))
            )
        );
        assert_eq!(
            said("[outrage LEAN_IN] What?")[0].delivery,
            Some(
                Delivery::new(Emotion::Outrage, DEFAULT_INTENSITY, Pace::Normal)
                    .with_gesture(Some(Gesture::LeanIn))
            )
        );
    }

    #[test]
    fn face_words_alone_are_a_neutral_cue() {
        let neutral = Delivery::new(Emotion::Neutral, DEFAULT_INTENSITY, Pace::Normal);
        assert_eq!(
            said("[glare] Kneel."),
            vec![Spoken {
                text: "Kneel.".into(),
                delivery: Some(neutral.with_gesture(Some(Gesture::Glare))),
            }]
        );
        assert_eq!(
            said("[eyes 0.4] So small.")[0],
            Spoken {
                text: "So small.".into(),
                delivery: Some(neutral.with_eye_light(Some(0.4))),
            }
        );
        assert_eq!(
            said("[nod eyes 1.5] Yes.")[0].delivery,
            Some(
                neutral
                    .with_gesture(Some(Gesture::Nod))
                    .with_eye_light(Some(1.5))
            )
        );
        // A bracket with any word that is not a face word stays markup and is stripped.
        assert_eq!(
            said("[glare at them] Kneel."),
            vec![Spoken::plain("Kneel.")]
        );
        assert_eq!(said("[eyes] Kneel."), vec![Spoken::plain("Kneel.")]);
        assert_eq!(
            said("[eyes 0.4 slow] Kneel."),
            vec![Spoken::plain("Kneel.")]
        );
        assert_eq!(said("[laughs] Kneel."), vec![Spoken::plain("Kneel.")]);
    }

    #[test]
    fn an_unknown_word_in_a_cue_is_ignored_and_the_cue_kept() {
        assert_eq!(
            said("[menace 0.9 wink] Kneel."),
            vec![Spoken {
                text: "Kneel.".into(),
                delivery: Some(Delivery::new(Emotion::Menace, 0.9, Pace::Normal)),
            }]
        );
        // `eyes` without a number is an unknown word too.
        assert_eq!(
            said("[menace eyes glare] Kneel.")[0].delivery,
            Some(
                Delivery::new(Emotion::Menace, DEFAULT_INTENSITY, Pace::Normal)
                    .with_gesture(Some(Gesture::Glare))
            )
        );
    }

    #[test]
    fn a_face_only_cue_neither_extends_nor_ends_an_emotion_run() {
        let n = Delivery::new(Emotion::Neutral, 0.5, Pace::Normal);
        let g = Delivery::new(Emotion::Glee, 0.5, Pace::Normal);
        let face = said("[glare] x")[0].delivery.unwrap();
        let eyes = said("[eyes 2] x")[0].delivery.unwrap();
        let mut run = CueRun::default();
        // Face-only cues alone never trip the check.
        let seen: Vec<bool> = [&face, &face, &eyes, &face]
            .into_iter()
            .map(|d| run.note(Some(d)))
            .collect();
        assert_eq!(seen, vec![false; 4]);
        // Between two glee lines a face-only cue does not reset the run: the third glee trips.
        let mut run = CueRun::default();
        let seen: Vec<bool> = [&g, &face, &g, &g]
            .into_iter()
            .map(|d| run.note(Some(d)))
            .collect();
        assert_eq!(seen, vec![false, false, false, true]);
        // An explicit neutral cue without face words still counts as before.
        let mut run = CueRun::default();
        let seen: Vec<bool> = [&n, &n, &n]
            .into_iter()
            .map(|d| run.note(Some(d)))
            .collect();
        assert_eq!(seen, vec![false, false, true]);
    }

    #[test]
    fn a_line_with_no_cue_is_unchanged() {
        assert_eq!(
            said("Wait ... what?  Kneel."),
            vec![Spoken::plain("Wait ... what?  Kneel.")]
        );
    }

    #[test]
    fn disallowed_op_is_rejected() {
        let reply = "OP: {\"op\":\"sign_off\",\"reason\":\"bored\"}";
        let lines = parse_reply(reply, &allowed(&["mark_moment"]), None, &prefixes());
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
            &prefixes(),
        );
        assert!(matches!(&lines[0], Line::Op { .. }));
        assert!(matches!(&lines[1], Line::Rejected { rule, .. } if rule == "op_parse"));
    }

    /// One value of every `Op` variant. `slot` is an exhaustive `match` with no wildcard, so
    /// a new variant fails to compile here until it is given a sample and an example.
    fn samples() -> Vec<Op> {
        fn slot(op: &Op) -> usize {
            match op {
                Op::Highlight { .. } => 0,
                Op::StartPoll { .. } => 1,
                Op::ClosePoll { .. } => 2,
                Op::SummonFoil { .. } => 3,
                Op::DismissFoil => 4,
                Op::RevealHex { .. } => 5,
                Op::DispatchResearch { .. } => 6,
                Op::ShowCard { .. } => 7,
                Op::MarkMoment { .. } => 8,
                Op::SwitchSegment { .. } => 9,
                Op::SignOff { .. } => 10,
                Op::ShowScreen { .. } => 11,
                Op::PinLink { .. } => 12,
            }
        }
        let s = |x: &str| x.to_string();
        let ops = vec![
            Op::Highlight { msg_id: s("m") },
            Op::StartPoll {
                poll_id: s("p"),
                question: s("q"),
                options: vec![s("a")],
                min_rank: Rank::Thrall,
                secs: 1,
            },
            Op::ClosePoll { poll_id: s("p") },
            Op::SummonFoil { foil: s("f") },
            Op::DismissFoil,
            Op::RevealHex {
                hex: s("HEX-01"),
                grade: EvidenceGrade::Rep,
            },
            Op::DispatchResearch {
                job_id: s("r"),
                brief: s("b"),
            },
            Op::ShowCard {
                card_id: s("c"),
                title: s("t"),
                body: s("b"),
            },
            Op::MarkMoment { label: s("l") },
            Op::SwitchSegment {
                to: SegmentId::Seg02aHexmap,
            },
            Op::SignOff { reason: s("r") },
            Op::ShowScreen {
                screen: ScreenId::Browser,
                record_id: Some(s("DOC-01")),
                focus: None,
            },
            Op::PinLink {
                from: s("HEX-04"),
                to: s("HEX-09"),
                rel: LinkRel::Paid,
                record_id: s("CLM-02"),
            },
        ];
        let slots: Vec<usize> = ops.iter().map(slot).collect();
        assert_eq!(slots, (0..ops.len()).collect::<Vec<_>>());
        ops
    }

    #[test]
    fn every_op_example_parses() {
        for op in samples() {
            let tag = op_tag(&op);
            let ex = op_example(&tag).unwrap_or_else(|| panic!("{tag}: no example"));
            let back: Op = serde_json::from_str(ex).unwrap_or_else(|e| panic!("{tag}: {e}"));
            assert_eq!(op_tag(&back), tag);
        }
    }

    #[test]
    fn note_with_a_header_is_never_spoken() {
        let reply = "Welcome, minion.
Memory candidates (not spoken): NOTE: v1 is new
Memory candidates:
Back to conquest.";
        let lines = parse_reply(reply, &[], None, &prefixes());
        let spoken: Vec<_> = lines
            .iter()
            .filter_map(|l| match l {
                Line::Say(s) => Some(s.text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(spoken, vec!["Welcome, minion.", "Back to conquest."]);
        assert!(lines
            .iter()
            .any(|l| matches!(l, Line::Note(n) if n == "v1 is new")));
    }
}
