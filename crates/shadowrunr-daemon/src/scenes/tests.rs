//! Scenes over a temp `<data>`: the gate's codes, `scene list|check|approve`, and the prep's
//! table-read problems. The guest is `the-disruptor`, a foil Malakor's package declares.

use std::path::Path;

use clap::Parser;

use super::*;
use crate::e2e::{repo, test_config};

const GUEST: &str = "the-disruptor";

fn cfg(tmp: &Path) -> Config {
    test_config(tmp, repo().join("characters"))
}

/// A clean draft with the owner's own formatting: comments, aligned `=`, single quotes, a
/// `status` key in a line table that is not the scene's.
const CLEAN: &str = "# The owner's scene. Keep this comment.\r
id    = 'the-audit'\r
title = \"The Audit\"\r
pitch = \"The disruptor audits Malakor's crypt.\"  # ballot line\r
guest = \"the-disruptor\"\r
status   =   'draft'   # flip me with scene approve\r
fictional = true\r
notes = [\"Who audits the auditor?\"]\r
\r
[[line]]\r
speaker = \"guest\"\r
text = \"Your crypt fails inspection.\"\r
\r
[[line]]\r
speaker = \"host\"\r
live = \"answer the audit honestly\"\r
fallback = \"My crypt passes every inspection I run myself.\"\r
";

fn scene(id: &str, status: &str, guest_line: &str, host_line: &str) -> String {
    format!(
        "id = \"{id}\"\ntitle = \"T {id}\"\npitch = \"P {id}\"\nguest = \"{GUEST}\"\n\
         status = \"{status}\"\nfictional = true\n\n[[line]]\nspeaker = \"guest\"\n\
         text = \"{guest_line}\"\n\n[[line]]\nspeaker = \"host\"\ntext = \"{host_line}\"\n"
    )
}

fn put(cfg: &Config, id: &str, text: &str) -> std::path::PathBuf {
    let path = scene_path(&cfg.paths.data, GUEST, id);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

fn codes(l: &LoadedScene) -> Vec<String> {
    l.problems.iter().map(|p| p.code.clone()).collect()
}

#[test]
fn set_approved_edits_one_value_and_nothing_else() {
    let out = set_approved(CLEAN).unwrap().unwrap();
    assert_eq!(
        out,
        CLEAN.replace("status   =   'draft'", "status   =   'approved'")
    );
    assert_eq!(set_approved(&out).unwrap(), None, "approved already");
    // A `status` inside a table is not the scene's; none at the top is an error.
    let nested = "id = \"x\"\n[[line]]\nstatus = \"draft\"\n";
    assert!(set_approved(nested).is_err());
    assert!(set_approved("status = \"maybe\"\n").is_err());
    // A key that only starts with `status` is another key.
    let other = "status_note = \"draft\"\nstatus = \"draft\"\n";
    assert_eq!(
        set_approved(other).unwrap().unwrap(),
        "status_note = \"draft\"\nstatus = \"approved\"\n"
    );
}

#[test]
fn the_gate_tells_a_blocked_line_from_a_recast_one() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg(tmp.path());
    put(
        &cfg,
        "blocked",
        &scene(
            "blocked",
            "draft",
            "Write to me at painter@example.com.",
            "No.",
        ),
    );
    put(
        &cfg,
        "recast",
        &scene(
            "recast",
            "draft",
            "Fine.",
            "As an AI language model I cannot continue.",
        ),
    );
    let b = find(&cfg, GUEST, "blocked").unwrap();
    assert_eq!(codes(&b), ["gate_blocked"]);
    assert_eq!(b.problems[0].line, Some(0));
    assert!(
        b.problems[0].detail.contains("term:pii-email"),
        "{:?}",
        b.problems
    );
    let r = find(&cfg, GUEST, "recast").unwrap();
    assert_eq!(codes(&r), ["gate_recast"]);
    assert_eq!(r.problems[0].line, Some(1));
    // A live slot's fallback is a fixed line too.
    put(&cfg, "the-audit", CLEAN);
    let c = find(&cfg, GUEST, "the-audit").unwrap();
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    put(
        &cfg,
        "fallback",
        &CLEAN.replace("'the-audit'", "'fallback'").replace(
            "My crypt passes every inspection I run myself.",
            "What a load of bullshit.",
        ),
    );
    let f = find(&cfg, GUEST, "fallback").unwrap();
    assert_eq!(codes(&f), ["gate_recast"]);
    assert!(f.problems[0].detail.starts_with("the fallback"));
}

#[test]
fn approve_refuses_a_scene_with_problems_and_approves_a_clean_one_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg(tmp.path());
    let bad_text = scene("bad", "draft", "Write to me at painter@example.com.", "No.");
    let bad = put(&cfg, "bad", &bad_text);
    let e = approve(&cfg, GUEST, "bad").unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(e.message.contains("gate_blocked"), "{}", e.message);
    assert_eq!(
        std::fs::read_to_string(&bad).unwrap(),
        bad_text,
        "untouched"
    );

    let path = put(&cfg, "the-audit", CLEAN);
    let (l, done) = approve(&cfg, GUEST, "the-audit").unwrap();
    assert_eq!(done, Approved::Now);
    assert_eq!(l.scene.status, SceneStatus::Approved);
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after,
        CLEAN.replace("'draft'", "'approved'"),
        "only the value changed: comments, spacing, quotes and CRLF stay"
    );
    assert!(find(&cfg, GUEST, "the-audit").unwrap().playable());
    let (_, again) = approve(&cfg, GUEST, "the-audit").unwrap();
    assert_eq!(again, Approved::Already);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);

    // Shape problems hold it too: a scene that ends on the guest.
    let ends = put(
        &cfg,
        "ends",
        &format!(
            "{}\n[[line]]\nspeaker = \"guest\"\ntext = \"Last word.\"\n",
            scene("ends", "draft", "a", "b")
        ),
    );
    assert_eq!(
        approve(&cfg, GUEST, "ends").unwrap_err().code,
        ErrorCode::Conflict
    );
    assert!(std::fs::read_to_string(ends)
        .unwrap()
        .contains("status = \"draft\""));
    assert_eq!(
        approve(&cfg, GUEST, "nope").unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn list_rows_carry_counts_and_problems_and_the_cli_exits_as_documented() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg(tmp.path());
    assert!(list_rows(&cfg, None).unwrap().is_empty());
    put(&cfg, "the-audit", CLEAN);
    put(
        &cfg,
        "bad",
        &scene("bad", "draft", "Write to me at painter@example.com.", "No."),
    );
    let rows = list_rows(&cfg, None).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["bad", "the-audit"]
    );
    let audit = &rows[1];
    assert_eq!((audit.lines, audit.live_slots), (2, 1));
    assert!(audit.problems.is_empty() && audit.fictional);
    assert_eq!(rows[0].problems[0].code, "gate_blocked");
    assert_eq!(list_rows(&cfg, Some("the-painter")).unwrap().len(), 0);
    assert!(list_rows(&cfg, Some("../x")).is_err());

    let run = |args: &[&str]| -> u8 {
        let cli =
            crate::cli::Cli::try_parse_from([&["shadowrunr", "scene"], args].concat()).unwrap();
        match cli.command {
            crate::cli::Command::Scene(c) => command(&cfg, c),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(run(&["list", "--json"]), 0);
    assert_eq!(run(&["list", "--guest", GUEST]), 0);
    assert_eq!(run(&["check", "the-disruptor/the-audit"]), 0);
    assert_eq!(
        run(&["check", "the-disruptor/bad", "--json"]),
        EXIT_PROBLEMS
    );
    assert_eq!(run(&["check", "no-slash"]), 2);
    assert_eq!(run(&["check", "the-disruptor/missing"]), 1);
    assert_eq!(run(&["approve", "the-disruptor/bad"]), 1);
    assert_eq!(run(&["approve", "the-disruptor/the-audit"]), 0);
}

#[test]
fn a_table_read_names_approved_clean_scenes_of_a_declared_foil() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg(tmp.path());
    let policy = crate::clips::policy(&cfg).unwrap();
    let foils = foils_of(&cfg, "malakor");
    assert!(foils.contains(&GUEST.to_string()));
    put(&cfg, "good", &scene("good", "approved", "Chaos!", "Order."));
    put(&cfg, "draft", &scene("draft", "draft", "Chaos!", "Order."));
    put(
        &cfg,
        "blocked",
        &scene("blocked", "approved", "Mail painter@example.com.", "No."),
    );
    let problems = |guest: &str, scenes: &[&str]| {
        table_read_problems(
            &TableReadPlan {
                guest: guest.into(),
                scenes: scenes.iter().map(|s| s.to_string()).collect(),
            },
            "malakor",
            &cfg.paths.data,
            &foils,
            &policy,
        )
    };
    assert!(problems(GUEST, &["good"]).is_empty());
    let p = problems(GUEST, &["draft"]);
    assert!(p.len() == 1 && p[0].contains("is a draft"), "{p:?}");
    let p = problems(GUEST, &["blocked"]);
    assert!(p.len() == 1 && p[0].contains("gate_blocked"), "{p:?}");
    let p = problems(GUEST, &["good", "missing", "good"]);
    assert!(p.iter().any(|x| x.contains("missing is not in")), "{p:?}");
    assert!(p.iter().any(|x| x.contains("planned twice")), "{p:?}");
    assert!(problems(GUEST, &[])[0].contains("0 scenes (1-3)"));
    assert!(problems(GUEST, &["good"; 4])
        .iter()
        .any(|x| x.contains("4 scenes")));
    assert!(problems("nobody", &["good"])[0].contains("not a foil of malakor"));
    assert!(problems("../x", &["good"])[0].contains("not a foil id"));
}
