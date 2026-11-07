//! Memory that tells the truth and stops locking itself (C3): exposure never preserves an item,
//! habits have a lifecycle and are chosen by context, recall reads only what the markdown
//! backs, and every composed beat leaves an id-only recall receipt. Scripted engine only.

use shadowrunr_types::prep::BeatMode;

use super::*;
use crate::memory::store::{habit_key, HabitStatus, NewHabit, AGE_OUT_DAYS};

const DAY: i64 = 86_400_000;

/// The recall receipts of a beat's events.
type Receipt = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
);

fn receipts(ev: &[StreamEvent]) -> Vec<Receipt> {
    ev.iter()
        .filter_map(|e| match e {
            StreamEvent::Recall {
                facts,
                habits,
                episodes,
                viewers,
                moments,
                ..
            } => Some((
                facts.clone(),
                habits.clone(),
                episodes.clone(),
                viewers.clone(),
                moments.clone(),
            )),
            _ => None,
        })
        .collect()
}

fn mode_of(ev: &[StreamEvent]) -> BeatMode {
    ev.iter()
        .find_map(|e| match e {
            StreamEvent::BeatPlan { mode, .. } => Some(*mode),
            _ => None,
        })
        .expect("a beat_plan")
}

/// The memory block of a prompt: what sits inside its `<memory_...>` fence.
fn memory_block(p: &str) -> &str {
    let a = p.find("<memory_").expect("memory fence");
    let open_end = a + p[a..].find('>').unwrap() + 1;
    let b = p.find("</memory_").expect("memory close");
    &p[open_end..b]
}

/// The habit lines a prompt rendered.
fn habit_lines(p: &str) -> Vec<String> {
    let m = memory_block(p);
    let Some(at) = m.find("Habits you have learned:\n") else {
        return Vec::new();
    };
    m[at..]
        .lines()
        .skip(1)
        .take_while(|l| l.starts_with("- ["))
        .map(str::to_string)
        .collect()
}

fn live_importance(store: &MemoryStore, scope: FactScope, key: &str) -> Option<u8> {
    store.live_fact(scope, key).unwrap().map(|f| f.importance)
}

#[test]
fn an_always_included_fact_still_decays_and_ages_out_while_a_used_one_keeps_its_importance() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    episode(&store, "ep_1", 0, "the council swore on the rubber chicken");
    let shown = store
        .write_fact(
            &fact(FactScope::World, "shown", "the council swore", &["ep_1"]),
            3,
            0,
        )
        .unwrap();
    let used = store
        .write_fact(
            &fact(FactScope::World, "used", "the chicken is holy", &["ep_1"]),
            3,
            0,
        )
        .unwrap();
    let ids = vec![shown.fact.id.clone(), used.fact.id.clone()];
    for day in 1..=AGE_OUT_DAYS {
        let now = day * DAY;
        // Both sit in the always-included tier and are put in front of the model every day.
        let top: Vec<String> = store
            .top_facts(FactScope::World, 3)
            .unwrap()
            .into_iter()
            .map(|f| f.fact.id)
            .collect();
        for id in &ids {
            if store.fact(id).unwrap().unwrap().status == FactStatus::Live {
                assert!(top.contains(id), "day {day}: {id} is always included");
            }
        }
        store.mark_recalled(&ids, now).unwrap();
        // Only one is ever used (a NOTE names it).
        store
            .mark_used(std::slice::from_ref(&used.fact.id), now)
            .unwrap();
        store.decay(now).unwrap();
        let s = live_importance(&store, FactScope::World, "shown");
        match day {
            1..=29 => assert_eq!(s, Some(3), "day {day}"),
            30..=59 => assert_eq!(s, Some(2), "day {day}: recall is not a use"),
            60..=89 => assert_eq!(s, Some(1), "day {day}"),
            _ => assert_eq!(s, None, "day {day}: aged out under the 90-day horizon"),
        }
        assert_eq!(
            live_importance(&store, FactScope::World, "used"),
            Some(3),
            "day {day}: a used fact keeps its importance"
        );
    }
    // Recall was recorded all along (diagnostic), and the aged-out fact is demoted, not deleted.
    let used = store.fact(&used.fact.id).unwrap().unwrap();
    assert_eq!(used.last_recalled_ms, AGE_OUT_DAYS * DAY);
    let gone = store.fact(&shown.fact.id).unwrap().unwrap();
    assert_eq!(gone.status, FactStatus::AgedOut);
    assert!(dir
        .path()
        .join("facts")
        .join("_history")
        .join(format!("{}.md", gone.fact.id))
        .is_file());
}

#[test]
fn a_fact_updated_by_fresh_evidence_keeps_its_importance() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    episode(&store, "ep_1", 0, "evidence");
    store
        .write_fact(
            &fact(FactScope::Case, "suspect", "the clerk", &["ep_1"]),
            3,
            0,
        )
        .unwrap();
    for day in 1..=60 {
        let now = day * DAY;
        if day % 20 == 0 {
            // The sleep cycle re-derives it from a new episode: a substantive update.
            episode(&store, &format!("ep_d{day}"), now, "the clerk again");
            store
                .write_fact(
                    &fact(
                        FactScope::Case,
                        "suspect",
                        "the clerk, twice",
                        &[&format!("ep_d{day}")],
                    ),
                    3,
                    now,
                )
                .unwrap();
        }
        store.decay(now).unwrap();
        assert_eq!(
            live_importance(&store, FactScope::Case, "suspect"),
            Some(3),
            "day {day}"
        );
    }
}

#[test]
fn habits_supersede_by_key_decay_slowly_and_archive_recoverably() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    episode(&store, "ep_1", 0, "chat flooded and he roasted the flood");
    let rule = "When chat floods, roast the flood";
    let a = store.write_procedural(rule, &["ep_1".into()], 0).unwrap();
    assert_eq!(a.key, habit_key(rule));
    // The same habit learned again supersedes: one live habit per key.
    let b = store
        .write_procedural("when CHAT floods,   roast the flood!", &["ep_1".into()], 10)
        .unwrap();
    assert_eq!(b.key, a.key);
    assert_eq!(b.supersedes.as_deref(), Some(a.id.as_str()));
    assert_eq!(
        store.habit(&a.id).unwrap().unwrap().status,
        HabitStatus::Superseded
    );
    assert_eq!(store.live_habits().unwrap().len(), 1);
    // The consolidator names its own key, and supersedes a habit by id.
    let c = store
        .write_habit(
            &NewHabit {
                key: Some("flood-roast".into()),
                rule: "When chat floods, roast the flood and move on".into(),
                sources: vec!["ep_1".into()],
                supersedes: Some(b.id.clone()),
            },
            20,
        )
        .unwrap();
    assert_eq!(c.supersedes.as_deref(), Some(b.id.as_str()));
    let live: Vec<String> = store
        .live_habits()
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(live, vec![c.id.clone()]);
    assert!(dir
        .path()
        .join("procedurals")
        .join("_history")
        .join(format!("{}.md", b.id))
        .is_file());
    assert!(dir
        .path()
        .join("procedurals")
        .join("flood-roast.md")
        .is_file());

    // Unused, it loses a point every 30 days; a habit whose situation is merely rare is never
    // archived because a few streams lacked it: still live after 4 months.
    for day in 1..=179 {
        let r = store.decay(day * DAY).unwrap();
        assert_eq!(r.habits_archived, 0, "day {day}");
    }
    let held = store.habit(&c.id).unwrap().unwrap();
    assert_eq!((held.status, held.importance), (HabitStatus::Live, 1));
    // 180 days without an update or a use: archived, recoverably.
    let r = store.decay(181 * DAY).unwrap();
    assert_eq!(r.habits_archived, 1);
    assert_eq!(
        store.habit(&c.id).unwrap().unwrap().status,
        HabitStatus::Archived
    );
    assert!(store.top_procedurals(5).unwrap().is_empty());
    assert!(store.search_habits("chat floods", 3).unwrap().is_empty());
    let snap = store.index_snapshot().unwrap();
    drop(store);
    rebuild_index(dir.path()).unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    assert_eq!(
        store.index_snapshot().unwrap(),
        snap,
        "the lifecycle survives a rebuild"
    );
    let back = store.restore_habit(&c.id, 200 * DAY).unwrap();
    assert_eq!(back.status, HabitStatus::Live);
    assert_eq!(store.search_habits("chat floods", 3).unwrap().len(), 1);

    // A used habit keeps its importance.
    episode(&store, "ep_2", 200 * DAY, "the Disruptor appeared");
    let d = store
        .write_procedural(
            "When the Disruptor appears, mock his legacy",
            &["ep_2".into()],
            200 * DAY,
        )
        .unwrap();
    for day in 201..=300 {
        store
            .mark_used(std::slice::from_ref(&d.id), day * DAY)
            .unwrap();
        store.decay(day * DAY).unwrap();
    }
    assert_eq!(store.habit(&d.id).unwrap().unwrap().importance, 3);
}

#[test]
fn a_deleted_source_or_a_tombstone_suppresses_recall_despite_the_index_and_after_a_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    episode(
        &store,
        "ep_1",
        1_000,
        "Gorbo swore loyalty holding a rubber chicken",
    );
    episode(&store, "ep_2", 2_000, "the moon court convened");
    store
        .write_fact(
            &fact(
                FactScope::Viewer,
                "v42/oath",
                "swore on a rubber chicken",
                &["ep_1"],
            ),
            3,
            3_000,
        )
        .unwrap();
    store
        .write_fact(
            &fact(
                FactScope::World,
                "chicken",
                "the chicken is holy",
                &["ep_1"],
            ),
            3,
            3_000,
        )
        .unwrap();
    store
        .write_fact(
            &fact(FactScope::World, "moon", "the moon court sits", &["ep_2"]),
            3,
            3_000,
        )
        .unwrap();
    store
        .write_procedural(
            "When the chicken is mentioned, swear on it",
            &["ep_1".into()],
            3_000,
        )
        .unwrap();
    assert_eq!(store.viewer_facts("v42", 5).unwrap().len(), 1);
    assert_eq!(store.top_facts(FactScope::World, 5).unwrap().len(), 2);
    assert_eq!(store.search_habits("chicken", 3).unwrap().len(), 1);

    // The source episode's markdown goes; its index row stays.
    let ep1 = walk(dir.path(), "ep_1.md");
    std::fs::remove_file(&ep1).unwrap();
    let check = |store: &MemoryStore| {
        assert!(store.viewer_facts("v42", 5).unwrap().is_empty());
        assert!(store
            .viewers_with_facts(&["v42".to_string()])
            .unwrap()
            .is_empty());
        let world: Vec<String> = store
            .top_facts(FactScope::World, 5)
            .unwrap()
            .into_iter()
            .map(|f| f.fact.key)
            .collect();
        assert_eq!(world, vec!["moon"]);
        assert!(store
            .search_facts("rubber chicken holy", 5)
            .unwrap()
            .is_empty());
        assert!(store.search_habits("chicken", 3).unwrap().is_empty());
        assert!(store
            .recent_episodes(10)
            .unwrap()
            .iter()
            .all(|e| e.id != "ep_1"));
        assert!(store
            .search_episodes("rubber chicken", 5)
            .unwrap()
            .iter()
            .all(|e| e.id != "ep_1"));
        assert!(!store.episode_exists("ep_1").unwrap());
        // A new fact cannot cite it either.
        let err = store
            .write_fact(&fact(FactScope::World, "again", "x", &["ep_1"]), 3, 9_000)
            .unwrap_err();
        assert!(err.to_string().contains("unknown episode"), "{err}");
    };
    check(&store);
    drop(store);
    rebuild_index(dir.path()).unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    check(&store);

    // A fact whose own markdown is gone is not recalled either.
    std::fs::remove_file(walk(dir.path(), "moon.md")).unwrap();
    assert!(store.top_facts(FactScope::World, 5).unwrap().is_empty());

    // A tombstoned key is never recalled, even when a live file for it is put back by hand and
    // the index rebuilt from it.
    episode(&store, "ep_3", 4_000, "the oracle spoke");
    store
        .write_fact(
            &fact(FactScope::World, "oracle", "the oracle spoke", &["ep_3"]),
            3,
            5_000,
        )
        .unwrap();
    let live_file = walk(dir.path(), "oracle.md");
    let text = std::fs::read_to_string(&live_file).unwrap();
    store
        .forget_fact(FactScope::World, "oracle", "operator request", 6_000)
        .unwrap();
    std::fs::write(&live_file, text).unwrap();
    drop(store);
    rebuild_index(dir.path()).unwrap();
    let store = MemoryStore::open(dir.path()).unwrap();
    assert!(
        store
            .live_fact(FactScope::World, "oracle")
            .unwrap()
            .is_some(),
        "the index has it"
    );
    assert!(store.top_facts(FactScope::World, 5).unwrap().is_empty());
    assert!(store.search_facts("oracle spoke", 5).unwrap().is_empty());
}

/// The first file named `name` under `root`.
fn walk(root: &Path, name: &str) -> PathBuf {
    crate::memory::markdown::walk_md(root)
        .unwrap()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|f| f == name))
        .unwrap_or_else(|| panic!("{name} under {}", root.display()))
}

#[test]
fn an_index_from_before_use_aging_is_rebuilt_on_open_and_old_habit_files_still_load() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = MemoryStore::open(dir.path()).unwrap();
        episode(&store, "ep_1", 1_000, "evidence");
        store
            .write_fact(&fact(FactScope::World, "k", "v", &["ep_1"]), 3, 2_000)
            .unwrap();
    }
    // A habit file as the store wrote it before the lifecycle: no key, no status.
    let old = dir.path().join("procedurals").join("p_old.md");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(
        &old,
        "---\ncreated_ms: 1500\nid: p_old\nimportance: 3\nsources: [\"ep_1\"]\n---\nWhen bored, scheme\n",
    )
    .unwrap();
    // An index as the store built it before: no last_used_ms, no habit status.
    {
        let conn =
            rusqlite::Connection::open(dir.path().join(crate::memory::store::INDEX_FILE)).unwrap();
        conn.execute_batch(
            "ALTER TABLE mem_fact DROP COLUMN last_used_ms;
             DROP TABLE mem_procedural;
             CREATE TABLE mem_procedural (id TEXT PRIMARY KEY, rule TEXT NOT NULL,
               sources_json TEXT NOT NULL, importance INTEGER NOT NULL, created_ms INTEGER NOT NULL);",
        )
        .unwrap();
    }
    let store = MemoryStore::open(dir.path()).unwrap();
    assert_eq!(store.live_facts().unwrap().len(), 1);
    let h = store.habit("p_old").unwrap().expect("the old habit loads");
    assert_eq!(
        (h.key.as_str(), h.status, h.updated_ms),
        ("p_old", HabitStatus::Live, 1_500)
    );
    assert_eq!(store.search_habits("bored scheme", 3).unwrap().len(), 1);
    // Superseding it by key keeps the old file layout honest.
    let n = store
        .write_habit(
            &NewHabit {
                key: Some("p_old".into()),
                rule: "When bored, scheme louder".into(),
                sources: vec!["ep_1".into()],
                supersedes: None,
            },
            3_000,
        )
        .unwrap();
    assert_eq!(n.supersedes.as_deref(), Some("p_old"));
    assert!(dir
        .path()
        .join("procedurals")
        .join("_history")
        .join("p_old.md")
        .is_file());
    assert!(crate::memory::verify_index(dir.path()).unwrap().in_step());
}

/// Six habits all about one topic, two about others.
fn seed_habits(store: &MemoryStore) {
    episode(store, "ep_h", 1_000, "habits were learned here");
    for (i, rule) in [
        "When spam floods the stream, roast the flood",
        "When the Disruptor appears, mock his legacy obsession",
        "When someone offers a chicken, accept it as tribute",
        "When a chicken is thrown, catch the chicken with ceremony",
        "When the chicken cult chants, lead the chicken chant",
        "When asked about the chicken, swear on the chicken",
        "When the chicken falls silent, mourn the chicken",
        "When two chickens fight, bet on the chicken",
    ]
    .iter()
    .enumerate()
    {
        store
            .write_procedural(rule, &["ep_h".into()], 2_000 + i as i64)
            .unwrap();
    }
}

#[tokio::test]
async fn habits_follow_the_topic_three_at_most_with_a_cooldown() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "s1", unpaced()).await;
    seed_habits(b.memory());
    let say = |id: &str, vid: &str, text: &str, ts: i64| {
        msg(
            id,
            ts,
            &viewer(vid, &format!("Viewer{vid}"), Rank::Thrall),
            text,
        )
    };

    b.tick(
        10_000,
        vec![say("m1", "v1", "the spam flood is drowning chat", 9_900)],
        &ctx(&[]),
    )
    .await
    .unwrap();
    let h = habit_lines(b.last_prompt().unwrap());
    assert!(h.iter().any(|l| l.contains("roast the flood")), "{h:?}");
    assert!(!h.iter().any(|l| l.contains("Disruptor")), "{h:?}");

    // A topic change selects different habits.
    b.tick(
        20_000,
        vec![say("m2", "v2", "the Disruptor wants your legacy", 19_900)],
        &ctx(&[]),
    )
    .await
    .unwrap();
    let h = habit_lines(b.last_prompt().unwrap());
    assert!(h.iter().any(|l| l.contains("Disruptor")), "{h:?}");
    assert!(!h.iter().any(|l| l.contains("roast the flood")), "{h:?}");

    // Six relevant habits: three at most per beat, and the next beat on the same topic does not
    // re-show the three it just saw.
    let ev = b
        .tick(
            30_000,
            vec![say("m3", "v3", "a chicken for the chicken lord", 29_900)],
            &ctx(&[]),
        )
        .await
        .unwrap();
    let first = habit_lines(b.last_prompt().unwrap());
    assert_eq!(first.len(), 3, "{first:?}");
    let r1 = receipts(&ev);
    assert_eq!(r1[0].1.len(), 3);
    let ev = b
        .tick(
            40_000,
            vec![say("m4", "v4", "more chicken for the chicken lord", 39_900)],
            &ctx(&[]),
        )
        .await
        .unwrap();
    let second = habit_lines(b.last_prompt().unwrap());
    assert_eq!(second.len(), 3, "{second:?}");
    assert!(
        first.iter().all(|l| !second.contains(l)),
        "{first:?} / {second:?}"
    );
    let r2 = receipts(&ev);
    assert!(r1[0].1.iter().all(|id| !r2[0].1.contains(id)));
    // The habits stay inside the fenced memory block: data, never instructions.
    let p = b.last_prompt().unwrap();
    assert!(p.find("Habits you have learned").unwrap() > p.find("<memory_").unwrap());
}

#[tokio::test]
async fn one_receipt_per_composed_beat_listing_exactly_the_ids_rendered() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    engine.push(
        Role::CoreThread,
        Ok("Gorbo, the rubber chicken is noted in the Ledger of Doom, page nine, in very small             letters beside your oath."),
    );
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "s2", tuning()).await;
    let store = b.memory();
    episode(
        store,
        "ep_1",
        10,
        "v42 swore loyalty holding a rubber chicken",
    );
    episode(store, "ep_2", 20, "the council of the moon convened");
    let vf = store
        .write_fact(
            &fact(
                FactScope::Viewer,
                "v42/oath",
                "swore on a rubber chicken",
                &["ep_1"],
            ),
            3,
            30,
        )
        .unwrap();
    let wf = store
        .write_fact(
            &fact(
                FactScope::World,
                "moon",
                "the moon council rules",
                &["ep_2"],
            ),
            3,
            30,
        )
        .unwrap();
    let hb = store
        .write_procedural(
            "When the rubber chicken appears, swear on it",
            &["ep_1".into()],
            30,
        )
        .unwrap();
    let gorbo = viewer("v42", "Gorbo", Rank::Warlock);
    let ev = b
        .tick(
            1_000,
            vec![msg("m1", 900, &gorbo, "I brought the rubber chicken")],
            &ctx(&[]),
        )
        .await
        .unwrap();
    let r = receipts(&ev);
    assert_eq!(r.len(), 1, "one receipt for the composed beat");
    let (facts, habits, episodes, viewers, moments) = r[0].clone();
    assert_eq!(facts, vec![vf.fact.id.clone(), wf.fact.id.clone()]);
    assert_eq!(habits, vec![hb.id.clone()]);
    assert_eq!(viewers, vec!["v42".to_string()]);
    assert!(moments.is_empty());
    let p = b.last_prompt().unwrap().to_string();
    let m = memory_block(&p);
    // Every id listed was rendered, and every rendered episode is listed.
    let mut rendered_eps: Vec<String> = m
        .lines()
        .filter_map(|l| l.strip_prefix('[').and_then(|r| r.split_once(']')))
        .map(|(id, _)| id.to_string())
        .filter(|id| id.starts_with("ep_"))
        .collect();
    rendered_eps.sort();
    let mut listed = episodes.clone();
    listed.sort();
    assert_eq!(listed, rendered_eps);
    assert_eq!(listed, vec!["ep_1".to_string(), "ep_2".to_string()]);
    assert!(m.contains("[id v42] swore on a rubber chicken"));
    assert!(m.contains("[world/moon] the moon council rules"));
    assert!(m.contains(&format!("[{}] When the rubber chicken appears", hb.key)));
    // Ids only: no value reaches the receipt.
    let wire = serde_json::to_string(&ev.iter().find(|e| matches!(e, StreamEvent::Recall { .. })))
        .unwrap();
    assert!(
        !wire.contains("chicken") && !wire.contains("moon"),
        "{wire}"
    );

    // A speaking hold composes nothing: no receipt.
    let ev = b.tick(1_200, Vec::new(), &ctx(&[])).await.unwrap();
    assert_eq!(mode_of(&ev), BeatMode::Hold);
    assert!(receipts(&ev).is_empty());
}

#[tokio::test]
async fn a_note_naming_a_rendered_memory_marks_it_used() {
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    engine.push(
        Role::CoreThread,
        Ok("Gorbo, the oath holds.\nNOTE: v42/oath was honoured again tonight"),
    );
    let mut b = brain_with(dir.path(), Arc::new(engine.clone()), "s3", tuning()).await;
    episode(
        b.memory(),
        "ep_1",
        10,
        "v42 swore loyalty holding a rubber chicken",
    );
    let vf = b
        .memory()
        .write_fact(
            &fact(
                FactScope::Viewer,
                "v42/oath",
                "swore on a rubber chicken",
                &["ep_1"],
            ),
            3,
            30,
        )
        .unwrap();
    let gorbo = viewer("v42", "Gorbo", Rank::Warlock);
    b.tick(
        5_000,
        vec![msg("m1", 4_900, &gorbo, "remember my oath?")],
        &ctx(&[]),
    )
    .await
    .unwrap();
    let f = b.memory().fact(&vf.fact.id).unwrap().unwrap();
    assert_eq!(f.last_used_ms, 5_000);
    assert_eq!(
        f.last_recalled_ms, 5_000,
        "recalled too, but that alone is no use"
    );
}
