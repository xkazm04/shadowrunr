//! End-to-end tests of the `shadowrunr` binary. Scripted engine only: no quota is spent.

use std::path::{Path, PathBuf};
use std::process::Command;

use shadowrunr_types::StreamEvent;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn lit(p: &Path) -> String {
    // TOML literal string: no escape processing, so Windows backslashes survive.
    format!("'{}'", p.display())
}

/// A shadowrunr.toml in `tmp` that reads the repo's packages and writes into `tmp`.
fn write_config(tmp: &Path, characters: &Path) {
    let r = repo();
    let text = format!(
        "[engine]\nscripted = true\n\n[paths]\ncharacters = {}\nruns = 'runs'\ndata = 'data'\npolicy = {}\nsim = {}\nsegments = {}\n\n[sim]\nmax_featured = 2\n",
        lit(characters),
        lit(&r.join("policy")),
        lit(&r.join("sim")),
        lit(&r.join("crates").join("shadowrunr-segments").join("data")),
    );
    std::fs::write(tmp.join("shadowrunr.toml"), text).unwrap();
}

fn shadowrunr(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_shadowrunr"))
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn sim_run_scripted_writes_spec_transcript_and_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(tmp.path(), &repo().join("characters"));
    // Run from a subdirectory: the config is discovered upward.
    let sub = tmp.path().join("nested");
    std::fs::create_dir_all(&sub).unwrap();
    let out = shadowrunr(
        &sub,
        &[
            "sim",
            "run",
            "--scripted",
            "--character",
            "malakor",
            "--tone",
            "theatrical",
            "--tier",
            "haiku",
            "--segment",
            "SEG-01",
            "--audience",
            "30",
            "--seed",
            "1",
            "--duration",
            "120",
        ],
    );
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let run_dir = tmp
        .path()
        .join("runs/sim/adhoc/theatrical.haiku.SEG-01.a30.s1");
    for f in ["spec.json", "transcript.jsonl", "ledger.jsonl"] {
        assert!(run_dir.join(f).is_file(), "missing {f}");
    }
    let text = std::fs::read_to_string(run_dir.join("transcript.jsonl")).unwrap();
    let events: Vec<StreamEvent> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect();
    assert!(matches!(
        events.first(),
        Some(StreamEvent::StreamStart { .. })
    ));
    assert!(matches!(events.last(), Some(StreamEvent::StreamEnd { .. })));
    assert!(events.len() > 2);
    let spec: shadowrunr_types::SimRunSpec =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("spec.json")).unwrap()).unwrap();
    assert_eq!(spec.audience_size, 30);
    assert_eq!(spec.duration_s, 120);
    assert_eq!(spec.population, "growing_channel");
    // Ledger rows are JSON objects, one per engine call.
    let ledger = std::fs::read_to_string(run_dir.join("ledger.jsonl")).unwrap();
    assert!(ledger.lines().count() > 0);
    for l in ledger.lines() {
        assert!(serde_json::from_str::<serde_json::Value>(l)
            .unwrap()
            .is_object());
    }
}

#[test]
fn preflight_exits_non_zero_on_a_red_check() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(tmp.path(), &tmp.path().join("no-such-characters"));
    let out = shadowrunr(tmp.path(), &["preflight", "--json"]);
    assert!(!out.status.success());
    let reports: Vec<shadowrunr_types::HealthReport> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reports.len(), 8);
    let pf02 = &reports[1];
    assert_eq!(pf02.check, "PF-02");
    assert_eq!(pf02.state, shadowrunr_types::HealthState::Red);
}

#[test]
fn preflight_passes_with_a_valid_package() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(tmp.path(), &repo().join("characters"));
    let out = shadowrunr(tmp.path(), &["preflight"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.lines().count(), 8, "{text}");
}

#[test]
fn descoped_segment_is_refused_by_the_cli() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(tmp.path(), &repo().join("characters"));
    let out = shadowrunr(
        tmp.path(),
        &["sim", "run", "--scripted", "--segment", "SEG-02B-I"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("descoped"));
}

#[test]
fn character_rules_cli_toggles_the_override_and_lists() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(tmp.path(), &repo().join("characters"));
    let out = shadowrunr(
        tmp.path(),
        &[
            "character",
            "rules",
            "malakor",
            "--disable",
            "no-financial-promotion",
            "--json",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rules: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rules[2]["id"], "no-financial-promotion");
    assert_eq!(rules[2]["enabled"], false);
    assert!(tmp
        .path()
        .join("data/characters/malakor/rules.json")
        .is_file());
    let out = shadowrunr(tmp.path(), &["character", "rules", "malakor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("on   crisis-handoff"), "{text}");
    assert!(text.contains("off  no-financial-promotion"), "{text}");
    assert!(text.contains("operator override; default on"), "{text}");
    let out = shadowrunr(
        tmp.path(),
        &["character", "rules", "malakor", "--enable", "nope"],
    );
    assert!(!out.status.success());
}

#[test]
fn policy_install_list_cli_copies_a_local_source_into_policy_local() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo();
    let policy = tmp.path().join("policy");
    std::fs::create_dir_all(&policy).unwrap();
    let text = format!(
        "[engine]\nscripted = true\n\n[paths]\ncharacters = {}\nruns = 'runs'\ndata = 'data'\npolicy = 'policy'\nsim = {}\nsegments = {}\n",
        lit(&r.join("characters")),
        lit(&r.join("sim")),
        lit(&r.join("crates").join("shadowrunr-segments").join("data")),
    );
    std::fs::write(tmp.path().join("shadowrunr.toml"), text).unwrap();
    // Invented tokens only.
    let src = tmp.path().join("words.json");
    std::fs::write(
        &src,
        r#"[{"word":"zorblax","categories":["slur_racial"]},{"word":"wibblet","categories":["insult"]}]"#,
    )
    .unwrap();
    let out = shadowrunr(
        tmp.path(),
        &[
            "policy",
            "install-list",
            "awdev1-better-profane-words",
            "--source",
            src.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("installed awdev1-better-profane-words (2 terms)"),
        "{stdout}"
    );
    let txt =
        std::fs::read_to_string(policy.join("local/awdev1-better-profane-words.txt")).unwrap();
    assert!(txt.contains("zorblax\tslur_racial"));
    let meta: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(policy.join("local/awdev1-better-profane-words.json")).unwrap(),
    )
    .unwrap();
    assert!(meta["licence"].as_str().unwrap().starts_with("GPL-3.0"));
    assert_eq!(meta["sha256"].as_str().unwrap().len(), 64);
    assert!(meta["installed"].as_str().unwrap().len() == 10);
}
