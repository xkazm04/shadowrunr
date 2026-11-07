//! Night two is real (L1): on-show moments written deterministically on night one, and one
//! cited recognition on a returning viewer's first pick of night two. One memory directory,
//! a brain per stream, the scripted `FakeEngine`.

use super::*;
use crate::moments::{moment_key, RECOGNITION_NOTE};

const DAY: i64 = 86_400_000;

fn propose(id: &str, ts: i64, v: &Viewer, text: &str) -> ChatMsg {
    let mut m = msg(id, ts, v, &format!("!propose {text}"));
    m.command = Some(Command::Propose { text: text.into() });
    m
}

fn pick_reason(ev: &[StreamEvent]) -> String {
    ev.iter()
        .find_map(|e| match e {
            StreamEvent::Picked { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn receipt_moments(ev: &[StreamEvent]) -> Option<Vec<String>> {
    ev.iter().find_map(|e| match e {
        StreamEvent::Recall { moments, .. } => Some(moments.clone()),
        _ => None,
    })
}

/// The memory fence of a prompt.
fn fence(p: &str) -> &str {
    let a = p.find("<memory_").expect("memory fence");
    let b = p.find("</memory_").expect("memory close");
    &p[a..b]
}

/// Night one: the warlock's proposal is heard on air, a crisis beat answers a disclosure, and
/// the stream ends. Returns the night's start of day.
async fn night_one(dir: &Path, warlock: &Viewer, soot: &Viewer) {
    let engine = FakeEngine::new();
    engine.push(
        Role::CoreThread,
        Ok("SwampLord, annexing the swamp hex is approved: the frogs will pay tax by Thursday."),
    );
    engine.push(
        Role::CoreThread,
        Ok("SootGoblin24, the bit can wait: please reach out to a local crisis line or someone you trust nearby.\nNOTE: v-soot said something heavy"),
    );
    let mut b = brain_with(dir, Arc::new(engine.clone()), "s1", unpaced()).await;
    let ev = b
        .tick(
            DAY + 10_000,
            vec![propose("p1", DAY + 9_900, warlock, "annex the swamp hex")],
            &ctx(&[]),
        )
        .await
        .unwrap();
    assert!(says(&ev)[0].contains("swamp"), "{ev:?}");
    b.tick(
        DAY + 20_000,
        vec![msg("d1", DAY + 19_900, soot, super::L167)],
        &ctx(&[]),
    )
    .await
    .unwrap();
    b.end_stream(DAY + 60_000).await.unwrap();
}

#[tokio::test]
async fn a_moment_from_night_one_is_recognised_once_on_night_two() {
    let dir = tempfile::tempdir().unwrap();
    let warlock = viewer("v-swamp", "SwampLord", Rank::Warlock);
    let soot = viewer("v-soot", "SootGoblin24", Rank::Thrall);
    night_one(dir.path(), &warlock, &soot).await;

    // Night one wrote the warlock's moment under its fixed key, cited by a live episode file.
    let store = MemoryStore::open(dir.path()).unwrap();
    let key = moment_key("v-swamp", "s1");
    let m = store
        .live_fact(FactScope::Viewer, &key)
        .unwrap()
        .expect("the proposal's moment");
    assert!(
        m.fact.value.starts_with("proposal heard · SEG-01 · "),
        "{}",
        m.fact.value
    );
    assert!(
        m.fact.value.contains("annexing the swamp hex"),
        "{}",
        m.fact.value
    );
    assert!(
        m.fact.value.contains("annex the swamp hex"),
        "{}",
        m.fact.value
    );
    let ep = m.fact.sources[0].clone();
    assert!(store.episode_exists(&ep).unwrap(), "the source is on disk");
    // The care beat wrote none, whatever its NOTE named.
    assert!(store
        .viewer_facts("v-soot", 10)
        .unwrap()
        .iter()
        .all(|f| !crate::moments::is_moment_key(&f.fact.key)));
    drop(store);

    // Night two, a day later: a fresh brain on the same memory.
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "s2", unpaced()).await;
    let t2 = 2 * DAY;
    let ev = b
        .tick(
            t2 + 10_000,
            vec![msg("n1", t2 + 9_900, &warlock, "hail, master, I return")],
            &ctx(&[]),
        )
        .await
        .unwrap();
    assert!(pick_reason(&ev).contains("regular"), "{}", pick_reason(&ev));
    let p = b.last_prompt().unwrap().to_string();
    let inside = fence(&p);
    assert!(
        inside.contains(&format!(
            "[id v-swamp] from a past night: [{key}] proposal heard"
        )),
        "{inside}"
    );
    assert!(inside.contains(&format!("(episode {ep})")), "{inside}");
    assert!(
        p.contains(RECOGNITION_NOTE),
        "the turn frames it as recognition"
    );
    let outside = p.replace(inside, "");
    assert!(
        !outside.contains("annex the swamp"),
        "the moment stays inside the fence"
    );
    assert_eq!(receipt_moments(&ev), Some(vec![m.fact.id.clone()]));

    // The second message of the night gets no moment.
    let ev = b
        .tick(
            t2 + 20_000,
            vec![msg(
                "n2",
                t2 + 19_900,
                &warlock,
                "what is tonight's scheme?",
            )],
            &ctx(&[]),
        )
        .await
        .unwrap();
    let p = b.last_prompt().unwrap();
    assert!(!p.contains("from a past night"), "{p}");
    assert!(!p.contains(RECOGNITION_NOTE));
    assert_eq!(receipt_moments(&ev), Some(Vec::new()));
}

#[tokio::test]
async fn a_deleted_source_or_a_tombstone_stops_a_moment() {
    let dir = tempfile::tempdir().unwrap();
    let warlock = viewer("v-swamp", "SwampLord", Rank::Warlock);
    let soot = viewer("v-soot", "SootGoblin24", Rank::Thrall);
    night_one(dir.path(), &warlock, &soot).await;
    let store = MemoryStore::open(dir.path()).unwrap();
    let m = store
        .live_fact(FactScope::Viewer, &moment_key("v-swamp", "s1"))
        .unwrap()
        .unwrap();
    // The source episode's markdown goes; the index still has it.
    let ep = crate::memory::markdown::walk_md(&dir.path().join("episodes"))
        .unwrap()
        .into_iter()
        .find(|p| {
            p.file_stem()
                .is_some_and(|s| s == m.fact.sources[0].as_str())
        })
        .unwrap();
    std::fs::remove_file(ep).unwrap();
    // A tombstoned key for night three is never written.
    store
        .forget_fact(
            FactScope::Viewer,
            &moment_key("v-swamp", "s3"),
            "viewer asked",
            2 * DAY,
        )
        .unwrap();
    drop(store);

    let engine = FakeEngine::new();
    engine.push(
        Role::CoreThread,
        Ok("SwampLord, the marsh tax stands; your new scheme is noted."),
    );
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "s3", unpaced()).await;
    let t3 = 3 * DAY;
    let ev = b
        .tick(
            t3 + 10_000,
            vec![propose("q1", t3 + 9_900, &warlock, "tax the marsh too")],
            &ctx(&[]),
        )
        .await
        .unwrap();
    assert!(
        !pick_reason(&ev).contains("regular"),
        "{}",
        pick_reason(&ev)
    );
    let p = b.last_prompt().unwrap();
    assert!(!p.contains("from a past night"), "{p}");
    assert_eq!(receipt_moments(&ev), Some(Vec::new()));
    // The proposal was heard, yet its tombstoned key stays unwritten.
    assert!(b
        .memory()
        .live_fact(FactScope::Viewer, &moment_key("v-swamp", "s3"))
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn a_stronger_act_supersedes_a_weaker_one_in_the_same_stream_never_the_reverse() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let v = viewer("v7", "MossGoblin", Rank::Warlock);
    engine.push(
        Role::CoreThread,
        Ok("MossGoblin, your oath is heard.\nNOTE: v7 swore an oath on the moss"),
    );
    engine.push(
        Role::CoreThread,
        Ok("MossGoblin, the moss crown scheme is approved, grudgingly."),
    );
    engine.push(Role::CoreThread, Ok("MossGoblin, yes, yes, noted again."));
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "s1", unpaced()).await;
    let key = moment_key("v7", "s1");
    b.tick(
        10_000,
        vec![msg("a1", 9_900, &v, "i swear an oath on the moss")],
        &ctx(&[]),
    )
    .await
    .unwrap();
    let noted = b
        .memory()
        .live_fact(FactScope::Viewer, &key)
        .unwrap()
        .unwrap();
    assert!(
        noted.fact.value.starts_with("noted"),
        "{}",
        noted.fact.value
    );
    b.tick(
        20_000,
        vec![propose("a2", 19_900, &v, "crown the moss")],
        &ctx(&[]),
    )
    .await
    .unwrap();
    let heard = b
        .memory()
        .live_fact(FactScope::Viewer, &key)
        .unwrap()
        .unwrap();
    assert!(
        heard.fact.value.starts_with("proposal heard"),
        "{}",
        heard.fact.value
    );
    assert_eq!(
        heard.fact.supersedes.as_deref(),
        Some(noted.fact.id.as_str())
    );
    // A weaker act later the same night does not replace it.
    engine.push(
        Role::CoreThread,
        Ok("MossGoblin again.\nNOTE: v7 is still talking about moss"),
    );
    b.tick(
        30_000,
        vec![msg("a3", 29_900, &v, "the moss remembers")],
        &ctx(&[]),
    )
    .await
    .unwrap();
    let still = b
        .memory()
        .live_fact(FactScope::Viewer, &key)
        .unwrap()
        .unwrap();
    assert_eq!(still.fact.id, heard.fact.id);
}
