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
    assert!(text.contains("lock  crisis-handoff"), "{text}");
    assert!(text.contains("on    crisis-chat-ban"), "{text}");
    assert!(text.contains("off   no-financial-promotion"), "{text}");
    assert!(text.contains("operator override; default on"), "{text}");
    let out = shadowrunr(
        tmp.path(),
        &["character", "rules", "malakor", "--enable", "nope"],
    );
    assert!(!out.status.success());
}

#[test]
fn character_rules_cli_refuses_to_disable_a_rail() {
    let tmp = tempfile::tempdir().unwrap();
    write_config(tmp.path(), &repo().join("characters"));
    let out = shadowrunr(
        tmp.path(),
        &[
            "character",
            "rules",
            "malakor",
            "--disable",
            "crisis-handoff",
        ],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("crisis-handoff is a rail"), "{err}");
    assert!(!tmp
        .path()
        .join("data/characters/malakor/rules.json")
        .exists());
}

#[test]
fn character_new_then_validate_passes_with_fill_warnings_and_validate_fails_on_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let chars = tmp.path().join("characters");
    std::fs::create_dir_all(&chars).unwrap();
    write_config(tmp.path(), &chars);
    let template = repo().join("characters").join("_template");
    let from = template.to_string_lossy().into_owned();
    let out = shadowrunr(tmp.path(), &["character", "new", "nova", "--from", &from]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("character.toml: identity.name: "), "{text}");
    assert!(text.contains("0 error(s)"), "{text}");
    assert!(chars.join("nova").join("character.toml").is_file());
    let out = shadowrunr(tmp.path(), &["character", "validate", "nova"]);
    assert!(out.status.success());
    // A second `new` refuses to overwrite.
    let out = shadowrunr(tmp.path(), &["character", "new", "nova", "--from", &from]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("already exists"));
    // An error fails validate (exit 1), named file: key: reason.
    let manifest = chars.join("nova").join("character.toml");
    let t = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        t.replace("default_tone = \"baseline\"", "default_tone = \"stormy\""),
    )
    .unwrap();
    let out = shadowrunr(tmp.path(), &["character", "validate", "nova"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("character.toml: default_tone: tones/stormy.md is missing"),
        "{text}"
    );
    // A package directory works as well as an id.
    let dir = repo().join("characters").join("malakor");
    let out = shadowrunr(
        tmp.path(),
        &["character", "validate", &dir.to_string_lossy()],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
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

// ── preflight --fix ─────────────────────────────────────────────────────

/// A port nothing listens on (bound once, then released).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// A pre-flight sandbox in `tmp`: the repo's characters and segments, its own policy (no
/// placeholder unless `rules` names one), `sim` (the repo's when `None`) and a daemon port.
fn write_fix_config(tmp: &Path, port: u16, rules: &str, sim: Option<&Path>) -> PathBuf {
    let r = repo();
    let policy = tmp.join("policy");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("platform-rules.json"), rules).unwrap();
    let sim = sim.map_or_else(|| r.join("sim"), Path::to_path_buf);
    let text = format!(
        "[engine]\nscripted = true\n\n[daemon]\nport = {port}\n\n[paths]\ncharacters = {}\nruns = 'runs'\ndata = 'data'\npolicy = 'policy'\nsim = {}\nsegments = {}\n",
        lit(&r.join("characters")),
        lit(&sim),
        lit(&r.join("crates").join("shadowrunr-segments").join("data")),
    );
    std::fs::write(tmp.join("shadowrunr.toml"), text).unwrap();
    policy
}

const NO_PLACEHOLDER_RULES: &str = r#"{ "version": 1, "platforms": {}, "term_rules": [] }"#;

/// Invented placeholder for a list with a known default source.
const KNOWN_LIST_RULES: &str = r#"{ "version": 1, "platforms": {}, "term_rules": [
  { "id": "slur-operator-list", "category": "slur",
    "pattern": "\\b\\B__OPERATOR_INSTALLED_LIST__awdev1-better-profane-words__slur_racial__",
    "platforms": ["twitch"], "action": "block" } ] }"#;

/// A memory store under `<tmp>/data/memory` whose index misses one markdown episode.
fn stale_memory(tmp: &Path) -> PathBuf {
    let mem = tmp.join("data").join("memory");
    let store = shadowrunr_brain::MemoryStore::open(&mem).unwrap();
    store
        .write_episode(&shadowrunr_types::Episode {
            id: "ep_1".into(),
            ts_ms: 1_000,
            stream_id: "s1".into(),
            kind: shadowrunr_types::EpisodeKind::Stream,
            text: "Gorbo begged for a title.".into(),
            importance: 3,
        })
        .unwrap();
    drop(store);
    let ep_dir = mem.join("episodes").join("1970").join("01");
    let text = std::fs::read_to_string(ep_dir.join("ep_1.md")).unwrap();
    std::fs::write(ep_dir.join("ep_2.md"), text.replace("ep_1", "ep_2")).unwrap();
    mem.join(shadowrunr_brain::memory::store::INDEX_FILE)
}

fn stamp(p: &Path) -> (Vec<u8>, std::time::SystemTime) {
    (
        std::fs::read(p).unwrap(),
        std::fs::metadata(p).unwrap().modified().unwrap(),
    )
}

fn row<'a>(text: &'a str, check: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(check))
        .unwrap_or_else(|| panic!("no {check} row in:\n{text}"))
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn preflight_fix_reindexes_a_stale_store_and_shows_before_after() {
    let tmp = tempfile::tempdir().unwrap();
    write_fix_config(tmp.path(), free_port(), NO_PLACEHOLDER_RULES, None);
    let index = stale_memory(tmp.path());
    let before = stamp(&index);
    let out = shadowrunr(tmp.path(), &["preflight", "--fix"]);
    let text = stdout(&out);
    assert!(
        out.status.success(),
        "{text}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        row(&text, "PF-03").contains("PF-03  amber -> green  fixed: memory reindex"),
        "{text}"
    );
    assert_ne!(stamp(&index), before, "the index was rebuilt");
}

/// A stand-in daemon on a free port: answers every `/rpc` with a daemon-shaped reply.
fn fake_daemon() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut c) = conn else { continue };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = c.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).into_owned();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            if k.eq_ignore_ascii_case("content-length") {
                                v.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let body = r#"{"result":[]}"#;
            let _ = write!(
                c,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

#[test]
fn preflight_fix_refuses_to_reindex_under_a_running_daemon() {
    let tmp = tempfile::tempdir().unwrap();
    let port = fake_daemon();
    write_fix_config(tmp.path(), port, NO_PLACEHOLDER_RULES, None);
    let index = stale_memory(tmp.path());
    let before = stamp(&index);
    let out = shadowrunr(tmp.path(), &["preflight", "--fix"]);
    let text = stdout(&out);
    assert_eq!(stamp(&index), before, "index bytes and mtime unchanged");
    assert!(
        row(&text, "PF-03").contains(&format!(
            "PF-03  amber  not fixed: the daemon on 127.0.0.1:{port} holds the index; stop it first"
        )),
        "{text}"
    );
}

#[test]
fn plain_preflight_writes_nothing_and_points_at_the_fix() {
    let tmp = tempfile::tempdir().unwrap();
    write_fix_config(tmp.path(), free_port(), NO_PLACEHOLDER_RULES, None);
    let index = stale_memory(tmp.path());
    let before = stamp(&index);
    let out = shadowrunr(tmp.path(), &["preflight"]);
    let text = stdout(&out);
    assert_eq!(stamp(&index), before, "plain pre-flight never repairs");
    assert!(
        row(&text, "PF-03").ends_with("fix: shadowrunr preflight --fix"),
        "{text}"
    );
}

#[test]
fn preflight_fix_never_downloads_without_download() {
    let tmp = tempfile::tempdir().unwrap();
    let policy = write_fix_config(tmp.path(), free_port(), KNOWN_LIST_RULES, None);
    let out = shadowrunr(tmp.path(), &["preflight", "--fix"]);
    let text = stdout(&out);
    assert!(!policy.join("local").exists(), "nothing installed: {text}");
    assert!(
        row(&text, "PF-05").contains(
            "needs --download: fetches awdev1-better-profane-words from https://raw.githubusercontent.com/awdev1/better-profane-words/main/words.json"
        ),
        "{text}"
    );
}

/// Every file under `dir` with its bytes and mtime.
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(tree(&p));
        } else {
            let (b, m) = stamp(&p);
            out.push((p, b, m));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn preflight_fix_json_carries_guided_and_external_remedies_and_changes_no_file() {
    let tmp = tempfile::tempdir().unwrap();
    let sim = tmp.path().join("empty-sim");
    std::fs::create_dir_all(&sim).unwrap();
    write_fix_config(tmp.path(), free_port(), NO_PLACEHOLDER_RULES, Some(&sim));
    let before = tree(tmp.path());
    let out = shadowrunr(tmp.path(), &["preflight", "--fix", "--json"]);
    assert_eq!(tree(tmp.path()), before, "the fix pass changed a file");
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", stdout(&out)));
    let rows = v["rows"].as_array().expect("rows");
    let by = |c: &str| rows.iter().find(|r| r["check"] == c).unwrap().clone();
    let pf06 = by("PF-06");
    assert_eq!(pf06["before"], "amber", "{pf06}");
    assert_eq!(pf06["remedy"]["kind"], "guided", "{pf06}");
    assert!(
        pf06["remedy"]["file"]
            .as_str()
            .unwrap()
            .ends_with("calibration.json"),
        "{pf06}"
    );
    let pf08 = by("PF-08");
    assert_eq!(pf08["remedy"]["kind"], "external", "{pf08}");
    assert!(pf08["remedy"]["text"].as_str().is_some(), "{pf08}");
}

#[test]
fn preflight_fix_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    write_fix_config(tmp.path(), free_port(), NO_PLACEHOLDER_RULES, None);
    stale_memory(tmp.path());
    let first = shadowrunr(tmp.path(), &["preflight", "--fix"]);
    assert!(first.status.success(), "{}", stdout(&first));
    let second = shadowrunr(tmp.path(), &["preflight", "--fix"]);
    let text = stdout(&second);
    assert!(second.status.success(), "{text}");
    assert!(text.contains("nothing to fix"), "{text}");
    assert!(!row(&text, "PF-03").contains("fixed"), "{text}");
}
