//! Ballot vectors: the runtime's poll rules, captured as data the screen must reproduce.
//!
//! Each scenario drives a real [`SegmentRuntime`] through chat and ops and records the
//! `StreamEvent`s a transcript would carry (each chat line, then what the runtime emitted for
//! it) and the open poll's tallies at the end. The stage-kit test
//! (`packages/stage-kit/src/ballots.test.ts`) folds the same events through the scene reducer
//! and asserts the screen's chat-observed tally equals these tallies. A screen-side gap the
//! project accepts is declared under `deviations`; any other divergence fails there.
//!
//! The golden file is compared byte for byte. `UPDATE_GOLDEN=1 cargo test -p
//! shadowrunr-segments --test ballot_vectors` rewrites it after a deliberate rule change.

use serde::Serialize;
use shadowrunr_segments::{parse_command, SegmentConfig, SegmentRuntime};
use shadowrunr_types::{
    ChatMsg, Command, Op, Platform, Rank, SegmentId, StreamEvent, Viewer, VoteTally,
};

const FOILS: [&str; 2] = ["the-disruptor", "the-life-coach"];

/// Where the screen reads the vectors.
fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/stage-kit/src/generated/ballot-vectors.json")
}

#[derive(Serialize)]
struct Vectors {
    #[serde(rename = "$comment")]
    comment: &'static str,
    scenarios: Vec<Scenario>,
    deviations: Vec<Deviation>,
}

#[derive(Serialize)]
struct Scenario {
    name: &'static str,
    what: &'static str,
    events: Vec<StreamEvent>,
    /// The poll open at the end.
    poll_id: String,
    /// The runtime's tallies of that poll at the end (equal to its last `vote` event).
    expected: Vec<VoteTally>,
}

/// A screen-side gap the project accepts: the screen's tally may differ on `scenario`.
#[derive(Serialize)]
struct Deviation {
    id: &'static str,
    scenario: &'static str,
    note: &'static str,
}

enum Step {
    /// A chat line whose `command` the platform adapter parsed from its text.
    Chat(&'static str, Rank, &'static str),
    /// A chat line carrying exactly this `command` (or none).
    Raw(&'static str, Rank, &'static str, Option<Command>),
    /// Chat lines the runtime ingests together, as one tick of a run does: the transcript logs
    /// every line before what the runtime emitted for the batch.
    Batch(Vec<(&'static str, Rank, &'static str)>),
    Op(Op),
}

fn viewer(id: &str, rank: Rank) -> Viewer {
    Viewer {
        id: id.into(),
        name: id.into(),
        platform: Platform::Sim,
        rank,
        sub_tier: None,
        months: 0,
        gifted: 0,
        is_mod: false,
    }
}

fn start_poll(id: &str, options: &[&str], min_rank: Rank) -> Step {
    Step::Op(Op::StartPoll {
        poll_id: id.into(),
        question: "Which way?".into(),
        options: options.iter().map(|s| s.to_string()).collect(),
        min_rank,
        secs: 60,
    })
}

fn vote(option: &str) -> Option<Command> {
    Some(Command::Vote {
        option: option.into(),
    })
}

/// Drive a fresh SEG-01 runtime (foils on, past its cold open) through `steps`.
fn run(name: &'static str, what: &'static str, steps: Vec<Step>) -> Scenario {
    let config = SegmentConfig {
        foils: FOILS.iter().map(|s| s.to_string()).collect(),
        cold_open: Some(shadowrunr_segments::table::ColdOpenRule {
            to: "opening".into(),
            min_s: 0,
            max_s: 0,
            active_chatters: 1,
            window_s: 60,
            sustain_s: 0,
        }),
        ..Default::default()
    };
    let mut rt = SegmentRuntime::with_config(SegmentId::Seg01ImprovisedRoom, config).unwrap();
    let mut events = rt.start(0);
    for (i, step) in steps.into_iter().enumerate() {
        let t = 1_000 * (i as i64 + 1);
        match step {
            Step::Op(op) => events.extend(rt.apply_ops(t, &[op])),
            Step::Chat(who, rank, text) => {
                let cmd = parse_command(text);
                events.extend(chat(&mut rt, t, i, who, rank, text, cmd));
            }
            Step::Raw(who, rank, text, cmd) => {
                events.extend(chat(&mut rt, t, i, who, rank, text, cmd));
            }
            Step::Batch(lines) => {
                let msgs: Vec<ChatMsg> = lines
                    .iter()
                    .enumerate()
                    .map(|(j, (who, rank, text))| ChatMsg {
                        id: format!("m{}-{}", i + 1, j + 1),
                        ts_ms: t,
                        viewer: viewer(who, *rank),
                        text: (*text).into(),
                        command: parse_command(text),
                    })
                    .collect();
                events.extend(msgs.iter().map(|msg| StreamEvent::Chat {
                    ts_ms: t,
                    msg: msg.clone(),
                }));
                events.extend(rt.ingest_chat(t, &msgs));
            }
        }
    }
    let poll_id = rt
        .open_poll_id()
        .unwrap_or_else(|| panic!("{name}: no poll open at the end"))
        .to_string();
    let expected = rt.open_poll_tallies().unwrap();
    // The tallies the screen is held to are the ones the runtime announced.
    if let Some(last) = events.iter().rev().find_map(|e| match e {
        StreamEvent::Vote {
            poll_id: p,
            tallies,
            ..
        } if *p == poll_id => Some(tallies),
        _ => None,
    }) {
        assert_eq!(
            serde_json::to_value(last).unwrap(),
            serde_json::to_value(&expected).unwrap(),
            "{name}: the last `vote` event disagrees with the open poll"
        );
    }
    Scenario {
        name,
        what,
        events,
        poll_id,
        expected,
    }
}

fn chat(
    rt: &mut SegmentRuntime,
    t: i64,
    i: usize,
    who: &str,
    rank: Rank,
    text: &str,
    command: Option<Command>,
) -> Vec<StreamEvent> {
    let msg = ChatMsg {
        id: format!("m{}", i + 1),
        ts_ms: t,
        viewer: viewer(who, rank),
        text: text.into(),
        command,
    };
    let mut out = vec![StreamEvent::Chat {
        ts_ms: t,
        msg: msg.clone(),
    }];
    out.extend(rt.ingest_chat(t, &[msg]));
    out
}

fn vectors() -> Vectors {
    use Rank::*;
    let scenarios = vec![
        run(
            "prefix_is_not_a_ballot",
            "A runtime-opened foil vote; `!vote the-d` is a unique prefix the runtime refuses.",
            vec![
                Step::Chat("a", Thrall, "!summon the-life-coach"),
                Step::Chat("b", Warlock, "!vote the-d"),
                Step::Chat("c", Thrall, "!vote the"),
            ],
        ),
        run(
            "summon_in_crowd_poll_is_not_a_ballot",
            "An op-opened crowd poll over the foils; `!summon` lands only in a foil vote.",
            vec![
                start_poll("p1", &FOILS, Thrall),
                Step::Chat("a", Thrall, "!summon the-disruptor"),
                Step::Chat("b", Cultist, "!summon the-life-coach"),
            ],
        ),
        run(
            "crowd_poll_ranks_case_index_revote",
            "Rank weights, min rank, case-insensitive text, a 1-based index and a re-vote.",
            vec![
                start_poll("p1", &["HEX-04", "HEX-03"], Acolyte),
                Step::Chat("a", DreadCouncil, "!vote HEX-04"),
                Step::Chat("b", Cultist, "!vote hex-03"),
                Step::Chat("c", Thrall, "!vote HEX-03"),
                Step::Chat("d", Acolyte, "!vote 2"),
                Step::Chat("b", Cultist, "!vote 1"),
            ],
        ),
        run(
            "summon_and_vote_in_foil_vote",
            "In a foil vote both `!summon` and `!vote` count; a second summon moves the ballot.",
            vec![
                Step::Chat("a", Thrall, "!summon the-life-coach"),
                Step::Chat("b", Warlock, "!summon The-Disruptor"),
                Step::Chat("c", Lich, "!vote 2"),
                Step::Chat("d", Acolyte, "!vote the-disruptor"),
                Step::Chat("a", Thrall, "!summon the-disruptor"),
                Step::Chat("e", Cultist, "!summon the-plumber"),
            ],
        ),
        run(
            "summon_batch_opens_foil_vote",
            "One batch opens a foil vote: lines before its first known-foil `!summon` find no poll; that line and every ballot after it count.",
            vec![
                Step::Batch(vec![
                    ("a", Warlock, "!vote the-disruptor"),
                    ("b", Thrall, "!summon the-plumber"),
                    ("c", Cultist, "!summon the-life-coach"),
                    ("d", Lich, "!vote 1"),
                    ("e", Acolyte, "!summon the-disruptor"),
                    ("c", Cultist, "!summon the-disruptor"),
                ]),
                Step::Chat("a", Warlock, "!vote 2"),
            ],
        ),
        run(
            "trimmed_exact_text",
            "Surrounding whitespace in the option is trimmed; inner text must match exactly.",
            vec![
                start_poll("p1", &["Left door", "Right door"], Thrall),
                Step::Raw("a", Thrall, "!vote", vote("  right DOOR \t")),
                Step::Raw("b", Warlock, "!vote", vote("left  door")),
                Step::Raw("c", Lich, "!vote", vote(" 1 ")),
            ],
        ),
        run(
            "unknown_option_and_out_of_range_index",
            "Unknown text, index 0 and an index past the end are not ballots.",
            vec![
                start_poll("p1", &["a", "b"], Thrall),
                Step::Chat("a", Thrall, "!vote nope"),
                Step::Chat("b", Warlock, "!vote 0"),
                Step::Chat("c", Lich, "!vote 3"),
                Step::Chat("d", Acolyte, "!vote b"),
            ],
        ),
        run(
            "index_before_text",
            "A number is an index first; only an out-of-range number falls back to exact text.",
            vec![
                start_poll("p1", &["2", "1", "30"], Thrall),
                Step::Chat("a", Thrall, "!vote 1"),
                Step::Chat("b", Warlock, "!vote 30"),
                Step::Chat("c", Lich, "!vote +3"),
                Step::Chat("d", Acolyte, "!vote 2"),
            ],
        ),
        run(
            "ascii_only_case_folding",
            "Case folds for ASCII letters only: `ü` does not match `Ü`.",
            vec![
                start_poll("p1", &["Über", "Unter"], Thrall),
                Step::Raw("a", Thrall, "!vote", vote("über")),
                Step::Raw("b", Cultist, "!vote", vote("UNTER")),
                Step::Raw("c", Warlock, "!vote", vote("Über")),
            ],
        ),
        run(
            "under_rank_revote_keeps_the_ballot",
            "A viewer below the min rank is refused; a re-vote naming no option keeps the earlier ballot.",
            vec![
                start_poll("p1", &["x", "y"], Cultist),
                Step::Chat("a", Thrall, "!vote x"),
                Step::Chat("b", Acolyte, "!vote y"),
                Step::Chat("c", Warlock, "!vote x"),
                Step::Chat("c", Warlock, "!vote nothing"),
                Step::Chat("d", DreadCouncil, "!vote y"),
            ],
        ),
        run(
            "zero_voter_row",
            "An option nobody chose stays in the tally with 0 voters and 0 weight.",
            vec![
                start_poll("p1", &["red", "green", "blue"], Thrall),
                Step::Chat("a", Thrall, "!vote red"),
                Step::Chat("b", Acolyte, "!vote 3"),
            ],
        ),
        run(
            "command_from_text",
            "A chat line with no parsed `command`: the runtime parses its text, the screen does not.",
            vec![
                start_poll("p1", &["a", "b"], Thrall),
                Step::Raw("a", Thrall, "!vote 1", None),
                Step::Chat("b", Warlock, "!vote b"),
            ],
        ),
        run(
            "crowd_poll_named_like_a_foil_vote",
            "A crowd poll the brain named `foil-vote-7`: the screen reads the id as a foil vote.",
            vec![
                start_poll("foil-vote-7", &FOILS, Thrall),
                Step::Chat("a", Thrall, "!summon the-disruptor"),
                Step::Chat("b", Warlock, "!vote 2"),
            ],
        ),
    ];
    Vectors {
        comment: "GENERATED by crates/shadowrunr-segments/tests/ballot_vectors.rs from the real SegmentRuntime. Do not edit: run `UPDATE_GOLDEN=1 cargo test -p shadowrunr-segments --test ballot_vectors`.",
        scenarios,
        deviations: vec![
            Deviation {
                id: "command_from_text",
                scenario: "command_from_text",
                note: "The screen counts only a chat line's parsed `command`; the runtime also parses a line's text when the adapter left `command` empty.",
            },
            Deviation {
                id: "poll_kind_from_id",
                scenario: "crowd_poll_named_like_a_foil_vote",
                note: "The stream does not carry a poll's kind: the screen recognises a foil vote by the runtime-minted `foil-vote-` id, so a crowd poll the brain named that way takes `!summon` on screen only.",
            },
        ],
    }
}

fn render() -> String {
    let mut out = serde_json::to_string_pretty(&vectors()).unwrap();
    out.push('\n');
    out
}

/// The first scenario (by name) whose content differs, or a description of the structural
/// difference.
fn first_difference(have: &str, want: &str) -> String {
    let parse = |s: &str| serde_json::from_str::<serde_json::Value>(s).ok();
    let (Some(h), Some(w)) = (parse(have), parse(want)) else {
        return "the file is not valid JSON".into();
    };
    let list = |v: &serde_json::Value| v["scenarios"].as_array().cloned().unwrap_or_default();
    let (hs, ws) = (list(&h), list(&w));
    for (i, ws) in ws.iter().enumerate() {
        match hs.get(i) {
            Some(hs) if hs == ws => {}
            _ => return format!("scenario `{}` differs", ws["name"].as_str().unwrap_or("?")),
        }
    }
    if hs.len() != ws.len() {
        return format!("{} scenarios on file, {} generated", hs.len(), ws.len());
    }
    if h["deviations"] != w["deviations"] {
        return "`deviations` differs".into();
    }
    "formatting differs".into()
}

#[test]
fn ballot_vectors_match_the_runtime() {
    let want = render();
    let path = golden_path();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        have == want,
        "{} is stale: {}. Rerun with UPDATE_GOLDEN=1 after a deliberate rule change.",
        path.display(),
        first_difference(&have, &want)
    );
}

#[test]
fn every_deviation_names_a_scenario() {
    let v = vectors();
    for d in &v.deviations {
        assert!(
            v.scenarios.iter().any(|s| s.name == d.scenario),
            "deviation {} names no scenario",
            d.id
        );
    }
}
