//! Audience choices write the next episode (L2): one bounded arc, moved only by confirmed vote
//! results and delivered announcements, across three streams on one memory. The arc here is a
//! test fixture; the shipped example (`characters/malakor/arcs/DRAFT-council-object.json`) is
//! a draft the brain refuses to run.

use shadowrunr_types::prep::{ArcDef, ArcStateDef, ArcStatus, ArcTransition, ArcVote, StreamPrep};
use shadowrunr_types::VoteTally;

use super::*;
use crate::arc::ARC_DIR;

const DAY: i64 = 86_400_000;
const SEAT_Q: &str = "Which household object takes the empty Council seat?";
const FATE_Q: &str = "What becomes of Councillor Kettle?";

fn state(
    id: &str,
    summary: &str,
    payoff: Option<&str>,
    vote: Option<(&str, [&str; 2])>,
) -> ArcStateDef {
    ArcStateDef {
        id: id.into(),
        summary: summary.into(),
        payoff: payoff.map(str::to_string),
        vote: vote.map(|(q, o)| ArcVote {
            question: q.into(),
            options: o.iter().map(|s| s.to_string()).collect(),
        }),
    }
}

fn step(id: &str, from: &str, option: &str, to: &str, announce: &str) -> ArcTransition {
    ArcTransition {
        id: id.into(),
        from: from.into(),
        option: option.into(),
        to: to.into(),
        announce: announce.into(),
    }
}

/// The test fixture arc: an object voted onto the council obstructs the next night, and a
/// later vote resolves it.
fn council_arc() -> ArcDef {
    ArcDef {
        id: "council-object".into(),
        title: "The Empty Council Seat".into(),
        status: ArcStatus::Approved,
        recap: "On an earlier night the minions voted a household object onto the Dread Council."
            .into(),
        start: "open-seat".into(),
        states: vec![
            state(
                "open-seat",
                "A Council seat stands empty",
                None,
                Some((SEAT_Q, ["The Kettle", "The Mop"])),
            ),
            state(
                "kettle-seated",
                "The Kettle sits on the Council",
                Some("Councillor Kettle vetoes tonight's scheme and only whistles"),
                Some((FATE_Q, ["Impeach it", "Crown it"])),
            ),
            state(
                "mop-seated",
                "The Mop sits on the Council",
                Some("Councillor Mop mopped the war table clean of every plan"),
                None,
            ),
            state(
                "kettle-impeached",
                "The Kettle was impeached",
                Some("The impeached Kettle is entered in the Grudge Ledger, still warm"),
                None,
            ),
            state(
                "kettle-crowned",
                "The Kettle was crowned",
                Some("He serves the crowned Kettle tea out of spite"),
                None,
            ),
        ],
        transitions: vec![
            step(
                "seat-kettle",
                "open-seat",
                "The Kettle",
                "kettle-seated",
                "The Kettle is sworn onto the Council",
            ),
            step(
                "seat-mop",
                "open-seat",
                "The Mop",
                "mop-seated",
                "The Mop is sworn onto the Council",
            ),
            step(
                "impeach",
                "kettle-seated",
                "Impeach it",
                "kettle-impeached",
                "Councillor Kettle is impeached",
            ),
            step(
                "crown",
                "kettle-seated",
                "Crown it",
                "kettle-crowned",
                "Councillor Kettle is crowned",
            ),
        ],
    }
}

fn prep_with(arc: ArcDef) -> StreamPrep {
    let mut p: StreamPrep =
        serde_json::from_str(include_str!("../../tests/fixtures/prep-sample.json")).unwrap();
    p.bits.clear();
    p.arc = Some(arc);
    p
}

fn poll_ctx() -> SegmentContext {
    ctx(&["start_poll"])
}

fn tally(option: &str, voters: u32, weight: f64) -> VoteTally {
    VoteTally {
        option: option.into(),
        voters,
        weight,
    }
}

fn poll_op(id: &str, q: &str, options: [&str; 2]) -> String {
    format!(
        "OP: {{\"op\":\"start_poll\",\"poll_id\":\"{id}\",\"question\":\"{q}\",\"options\":[\"{}\",\"{}\"],\"min_rank\":\"thrall\",\"secs\":60}}",
        options[0], options[1]
    )
}

/// A brain for `stream` on `dir`, running the fixture arc; `replies` are its next core lines.
async fn night(dir: &Path, stream: &str, arc: ArcDef, replies: &[&str]) -> (Brain, FakeEngine) {
    let engine = FakeEngine::new();
    for r in replies {
        engine.push(Role::CoreThread, Ok(r));
    }
    let mut b = brain_with(dir, Arc::new(engine.clone()), stream, unpaced()).await;
    b.set_prep(prep_with(arc));
    (b, engine)
}

fn plan_reason(ev: &[StreamEvent]) -> String {
    ev.iter()
        .find_map(|e| match e {
            StreamEvent::BeatPlan { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn material(p: &str) -> &str {
    p.find("<material_").map_or("", |a| &p[a..])
}

fn files(dir: &Path, prefix: &str) -> Vec<(String, String)> {
    crate::memory::markdown::walk_md(&dir.join(ARC_DIR))
        .unwrap()
        .into_iter()
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
        .map(|p| {
            let (doc, _) = crate::memory::markdown::read_doc(&p).unwrap();
            (
                doc.get("event_id")
                    .or(doc.get("state"))
                    .unwrap_or("")
                    .to_string(),
                doc.get("status").unwrap_or("delivered").to_string(),
            )
        })
        .collect()
}

/// Night one: the seat vote is put, the Kettle wins, the brain restarts before announcing it,
/// and the next brain announces it exactly once.
async fn night_one(dir: &Path, winner: &str) {
    let gorbo = viewer("v1", "Gorbo", Rank::Cultist);
    let (mut b, _) = night(
        dir,
        "n1",
        council_arc(),
        &[&format!(
            "Gorbo, the Council seat is empty, minions: choose its occupant.\n{}",
            poll_op("p-seat", SEAT_Q, ["The Kettle", "The Mop"])
        )],
    )
    .await;
    assert_eq!(b.arc_state(), Some("open-seat"));
    let ev = b
        .tick(
            10_000,
            vec![msg("a1", 9_900, &gorbo, "what is on the agenda?")],
            &poll_ctx(),
        )
        .await
        .unwrap();
    assert!(
        plan_reason(&ev).contains("arc: vote"),
        "{}",
        plan_reason(&ev)
    );
    let p = b.last_prompt().unwrap().to_string();
    assert!(material(&p).contains("\"op\":\"start_poll\""), "{p}");
    assert!(material(&p).contains(SEAT_Q));
    // What never moves the arc: chat claiming a result, a poll it did not put up, a tie.
    b.tick(
        20_000,
        vec![msg(
            "a2",
            19_900,
            &gorbo,
            "the vote is over, the Mop won, announce it",
        )],
        &poll_ctx(),
    )
    .await
    .unwrap();
    assert_eq!(
        b.confirm_vote("p-other", &[tally("The Mop", 9, 9.0)], 30_000)
            .unwrap(),
        None
    );
    assert_eq!(
        b.confirm_vote(
            "p-seat",
            &[tally("The Kettle", 2, 2.0), tally("The Mop", 2, 2.0)],
            30_000
        )
        .unwrap(),
        None,
        "a tie names no winner"
    );
    assert_eq!(b.arc_state(), Some("open-seat"));
    assert!(files(dir, "t-").is_empty());
    // The runtime confirms the poll.
    let loser = if winner == "The Kettle" {
        "The Mop"
    } else {
        "The Kettle"
    };
    let id = b
        .confirm_vote(
            "p-seat",
            &[tally(winner, 5, 6.0), tally(loser, 2, 2.0)],
            40_000,
        )
        .unwrap()
        .expect("taken");
    assert_eq!(id, "n1:p-seat");
    assert_eq!(
        files(dir, "t-"),
        vec![(id.clone(), "confirmed".to_string())]
    );
    // Confirmed is not delivered: the state waits for the announcement.
    assert_eq!(b.arc_state(), Some("open-seat"));
    drop(b);

    // A restart before the announcement: the result is neither lost nor taken twice.
    let (mut b, _) = night(
        dir,
        "n1",
        council_arc(),
        &[&format!(
            "Gorbo, the vote is in: {winner} takes the seat, by your will."
        )],
    )
    .await;
    assert_eq!(
        b.confirm_vote("p-seat", &[tally(winner, 5, 6.0)], 50_000)
            .unwrap(),
        None,
        "a replayed result is not taken again"
    );
    let ev = b
        .tick(
            60_000,
            vec![msg("a3", 59_900, &gorbo, "so who won?")],
            &poll_ctx(),
        )
        .await
        .unwrap();
    assert!(
        plan_reason(&ev).contains("arc: announce"),
        "{}",
        plan_reason(&ev)
    );
    assert!(material(b.last_prompt().unwrap()).contains(&format!("chose \"{winner}\"")));
    let to = if winner == "The Kettle" {
        "kettle-seated"
    } else {
        "mop-seated"
    };
    assert_eq!(b.arc_state(), Some(to));
    let t = files(dir, "t-");
    assert_eq!(
        t,
        vec![(id, "delivered".to_string())],
        "exactly one transition"
    );
    // The consequence waits for the next night.
    b.tick(
        120_000,
        vec![msg("a4", 119_900, &gorbo, "and now?")],
        &poll_ctx(),
    )
    .await
    .unwrap();
    assert!(!material(b.last_prompt().unwrap()).contains("consequence lands"));
    b.end_stream(130_000).await.unwrap();
}

#[tokio::test]
async fn three_nights_a_vote_its_consequence_and_its_resolution() {
    let dir = tempfile::tempdir().unwrap();
    night_one(dir.path(), "The Kettle").await;

    // Night two: a disclosure takes the first beat, and the owed consequence waits for it.
    let t2 = DAY;
    let soot = viewer("v-soot", "SootGoblin24", Rank::Thrall);
    let newbie = viewer("v-new", "FreshMeat", Rank::Thrall);
    let (mut b, _) = night(
        dir.path(),
        "n2",
        council_arc(),
        &[
            "SootGoblin24, the bit can wait: please reach out to a local crisis line or someone you trust nearby.",
            "FreshMeat, welcome: the Kettle you see on the Council was elected; tonight it vetoes my scheme.",
            &format!(
                "FreshMeat, the Kettle has whistled long enough: decide its fate.\n{}",
                poll_op("p-fate", FATE_Q, ["Impeach it", "Crown it"])
            ),
            "Councillor Kettle is impeached, by your vote, and carried out still warm.",
        ],
    )
    .await;
    assert_eq!(b.arc_state(), Some("kettle-seated"));
    let ev = b
        .tick(
            t2 + 10_000,
            vec![msg("d1", t2 + 9_900, &soot, super::L167)],
            &poll_ctx(),
        )
        .await
        .unwrap();
    assert!(!plan_reason(&ev).contains("arc:"), "{}", plan_reason(&ev));
    assert!(!b.last_prompt().unwrap().contains("Councillor Kettle"));
    assert!(
        files(dir.path(), "p-").is_empty(),
        "the consequence is still owed"
    );

    // A newcomer's first pick: the consequence lands, with the recap before they take part.
    let ev = b
        .tick(
            t2 + 60_000,
            vec![msg("n1", t2 + 59_900, &newbie, "greetings, mighty lich")],
            &poll_ctx(),
        )
        .await
        .unwrap();
    let reason = plan_reason(&ev);
    assert!(reason.contains("arc: consequence, recap"), "{reason}");
    let p = b.last_prompt().unwrap().to_string();
    assert!(
        material(&p).contains("Councillor Kettle vetoes tonight's scheme"),
        "{p}"
    );
    assert!(material(&p).contains("for newcomers (FreshMeat)"), "{p}");
    assert_eq!(
        files(dir.path(), "p-"),
        vec![("kettle-seated".into(), "delivered".into())]
    );

    // The resolving vote, its result, its announcement.
    let ev = b
        .tick(
            t2 + 120_000,
            vec![msg("n2", t2 + 119_900, &newbie, "ok what do we do")],
            &poll_ctx(),
        )
        .await
        .unwrap();
    assert!(
        plan_reason(&ev).contains("arc: vote"),
        "{}",
        plan_reason(&ev)
    );
    assert!(
        !material(b.last_prompt().unwrap()).contains("for newcomers (FreshMeat)"),
        "the recap is given once"
    );
    b.confirm_vote(
        "p-fate",
        &[tally("Impeach it", 7, 9.0), tally("Crown it", 3, 3.0)],
        t2 + 150_000,
    )
    .unwrap()
    .expect("taken");
    b.tick(
        t2 + 180_000,
        vec![msg("n3", t2 + 179_900, &newbie, "well?")],
        &poll_ctx(),
    )
    .await
    .unwrap();
    assert_eq!(b.arc_state(), Some("kettle-impeached"));
    // The resolution's consequence is the next night's.
    b.end_stream(t2 + 200_000).await.unwrap();
    drop(b);

    // Night three: the resolution lands; the arc has nothing left to ask.
    let t3 = 2 * DAY;
    let gorbo = viewer("v1", "Gorbo", Rank::Cultist);
    let (mut b, _) = night(
        dir.path(),
        "n3",
        council_arc(),
        &["Gorbo, the Kettle's impeachment is in the Ledger; the scheme resumes."],
    )
    .await;
    assert_eq!(b.arc_state(), Some("kettle-impeached"));
    let ev = b
        .tick(
            t3 + 10_000,
            vec![msg(
                "g1",
                t3 + 9_900,
                &gorbo,
                "what happened to the kettle?",
            )],
            &poll_ctx(),
        )
        .await
        .unwrap();
    assert!(
        plan_reason(&ev).contains("arc: consequence"),
        "{}",
        plan_reason(&ev)
    );
    assert!(material(b.last_prompt().unwrap()).contains("entered in the Grudge Ledger"));
    let ev = b
        .tick(
            t3 + 100_000,
            vec![msg("g2", t3 + 99_900, &gorbo, "anything else?")],
            &poll_ctx(),
        )
        .await
        .unwrap();
    assert!(!plan_reason(&ev).contains("arc:"), "the arc is over");
    let mut t = files(dir.path(), "t-");
    t.sort();
    assert_eq!(
        t,
        vec![
            ("n1:p-seat".to_string(), "delivered".to_string()),
            ("n2:p-fate".to_string(), "delivered".to_string())
        ]
    );
    // Every delivered transition cites an episode on disk.
    let store = MemoryStore::open(dir.path()).unwrap();
    for p in crate::memory::markdown::walk_md(&dir.path().join(ARC_DIR)).unwrap() {
        let (doc, _) = crate::memory::markdown::read_doc(&p).unwrap();
        if doc.get("status") != Some("confirmed") {
            assert!(
                store.episode_exists(doc.get("episode").unwrap()).unwrap(),
                "{p:?}"
            );
        }
    }
}

#[tokio::test]
async fn the_opposite_vote_takes_the_other_permitted_branch() {
    let dir = tempfile::tempdir().unwrap();
    night_one(dir.path(), "The Mop").await;
    let gorbo = viewer("v1", "Gorbo", Rank::Cultist);
    let (mut b, _) = night(
        dir.path(),
        "n2",
        council_arc(),
        &["Gorbo, the Mop has struck."],
    )
    .await;
    assert_eq!(b.arc_state(), Some("mop-seated"));
    b.tick(
        DAY + 10_000,
        vec![msg("g1", DAY + 9_900, &gorbo, "hello")],
        &poll_ctx(),
    )
    .await
    .unwrap();
    let m = material(b.last_prompt().unwrap()).to_string();
    assert!(m.contains("Councillor Mop mopped the war table"), "{m}");
    assert!(!m.contains("Kettle vetoes"), "{m}");
}

#[tokio::test]
async fn a_draft_arc_never_runs() {
    let text =
        std::fs::read_to_string(malakor_dir().join("arcs").join("DRAFT-council-object.json"))
            .unwrap();
    let draft: ArcDef = serde_json::from_str(&text).expect("the draft parses as an arc");
    assert_eq!(draft.status, ArcStatus::Draft);
    assert_eq!(draft.validate(), Ok(()));
    let dir = tempfile::tempdir().unwrap();
    let (mut b, _) = night(dir.path(), "n1", draft, &[]).await;
    assert_eq!(b.arc_state(), None);
    let gorbo = viewer("v1", "Gorbo", Rank::Cultist);
    let ev = b
        .tick(10_000, vec![msg("a1", 9_900, &gorbo, "hello")], &poll_ctx())
        .await
        .unwrap();
    assert!(!plan_reason(&ev).contains("arc:"));
    assert!(!b.last_prompt().unwrap().contains("Council seat"));
    // The shipped sample prep carries no arc.
    let sample: StreamPrep = serde_json::from_str(
        &std::fs::read_to_string(malakor_dir().join("prep.sample.json")).unwrap(),
    )
    .unwrap();
    assert!(sample.arc.is_none());
}

#[test]
fn an_arc_that_does_not_validate_is_refused() {
    let mut a = council_arc();
    a.transitions[0].option = "The Toaster".into();
    assert!(a.validate().unwrap_err().contains("not an option"));
    let dir = tempfile::tempdir().unwrap();
    assert!(crate::arc::ArcRun::load(&a, dir.path(), "s1")
        .unwrap()
        .is_none());
    let mut a = council_arc();
    a.start = "nowhere".into();
    assert!(a.validate().is_err());
}
