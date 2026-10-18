//! `character.list`, `character.get`, `segments.tables`, `ranks.table`, the method table and
//! the reconcile types, over `/rpc`.

use serde_json::{json, Value};
use shadowrunr_types::catalog::{CharacterDetail, CharacterSummary, RankTableInfo, SegmentTables};
use shadowrunr_types::reconcile::{Directive, PlanOption, ReconcileReport};
use shadowrunr_types::rpc::method_names;

use super::{has_null, repo, rpc, start, test_config};

fn typed<T: serde::de::DeserializeOwned>(v: &Value) -> T {
    serde_json::from_value(v["result"].clone()).unwrap_or_else(|e| panic!("{e}: {v}"))
}

#[tokio::test]
async fn character_list_and_get_expose_the_package() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;

    let list: Vec<CharacterSummary> = typed(&rpc(&addr, "character.list", json!({})).await);
    let m = list
        .iter()
        .find(|c| c.id == "malakor")
        .expect("malakor listed");
    assert_eq!(m.name, "Arch-Lich Malakor");
    assert_eq!(m.default_tone, "theatrical");
    assert!(
        m.tones.contains(&"weary-middle-manager".to_string()),
        "{:?}",
        m.tones
    );
    assert_eq!(m.foils.len(), 5);
    assert_eq!((m.rules_enabled, m.rules_total), (2, 2));

    let v = rpc(&addr, "character.get", json!({ "id": "malakor" })).await;
    let d: CharacterDetail = typed(&v);
    let paths: Vec<&str> = d.files.iter().map(|f| f.path.as_str()).collect();
    for p in [
        "character.toml",
        "constitution.md",
        "tones/theatrical.md",
        "rules/crisis-handoff.md",
    ] {
        assert!(paths.contains(&p), "{p} in {paths:?}");
    }
    assert!(d.files.iter().all(|f| f.bytes > 0));
    assert!(!d.constitution.trim().is_empty());
    assert!(d.bible.is_some());
    assert_eq!(d.tones.len(), m.tones.len());
    assert!(d.tones.iter().all(|t| !t.text.is_empty()));
    assert_eq!(d.foils.len(), 5);
    assert!(d.foils.iter().all(|f| f.declared));
    assert!(!d.situations.is_empty());
    let rule = d.rules.iter().find(|r| r.id == "crisis-handoff").unwrap();
    assert!(rule.enabled && rule.default && !rule.text.is_empty());
    assert_eq!(
        d.rank_titles
            .get(&shadowrunr_types::Rank::Lich)
            .map(String::as_str),
        Some("Lesser Lich")
    );
    // Optional fields are omitted, never null.
    assert!(!has_null(&v), "{v}");

    let v = rpc(&addr, "character.get", json!({ "id": "nobody" })).await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
    let v = rpc(&addr, "character.get", json!({ "id": "../x" })).await;
    assert_eq!(v["error"]["code"], "invalid_params", "{v}");
}

#[tokio::test]
async fn character_tone_set_overrides_the_active_tone_in_data_only() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    let list: Vec<CharacterSummary> = typed(&rpc(&addr, "character.list", json!({})).await);
    let m = list.iter().find(|c| c.id == "malakor").unwrap();
    assert_eq!(m.active_tone, m.default_tone, "no override: the default");

    let v = rpc(
        &addr,
        "character.tone.set",
        json!({ "id": "malakor", "tone": "weary-middle-manager" }),
    )
    .await;
    let s: CharacterSummary = typed(&v);
    assert_eq!(s.active_tone, "weary-middle-manager");
    assert_eq!(s.default_tone, "theatrical", "the package is untouched");
    let d: CharacterDetail = typed(&rpc(&addr, "character.get", json!({ "id": "malakor" })).await);
    assert_eq!(d.active_tone, "weary-middle-manager");
    assert!(tmp
        .path()
        .join("data/characters/malakor/active.json")
        .is_file());

    let v = rpc(
        &addr,
        "character.tone.set",
        json!({ "id": "malakor", "tone": "no-such-tone" }),
    )
    .await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
}

#[tokio::test]
async fn segments_tables_and_ranks_table_are_the_running_data() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;

    let t: SegmentTables = typed(&rpc(&addr, "segments.tables", json!({})).await);
    assert_eq!(t.segments.len(), 4);
    assert_eq!(t.op_tags.len(), 11);
    assert_eq!(t.command_names.len(), 5);
    let names: Vec<&str> = t.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, t.command_names);
    assert!(t
        .commands
        .iter()
        .all(|c| !c.args.is_empty() && !c.what.is_empty()));
    let gated: Vec<&str> = t
        .commands
        .iter()
        .filter(|c| c.min_rank.is_some())
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(gated, vec!["propose"]);
    let seg01 = &t.segments[0];
    assert_eq!(seg01.title, "Improvised Room");
    assert_eq!(seg01.initial, "opening");
    assert!(seg01.allowed_ops.contains(&"summon_foil".to_string()));
    assert_eq!(seg01.commands, ["summon", "propose", "vote"]);
    assert_eq!(seg01.clock.handover.as_ref().map(|h| h.ccv_min), Some(15));
    assert!(seg01
        .transitions
        .iter()
        .any(|r| r.from == "*" && r.on == "handover_due" && r.to == "handover_pending"));
    assert!(seg01
        .transitions
        .iter()
        .any(|r| r.guard.as_deref() == Some("foil_active")));
    let descoped = t
        .segments
        .iter()
        .filter(|s| s.design_notes.is_some())
        .count();
    assert_eq!(descoped, 2);

    let r: RankTableInfo = typed(&rpc(&addr, "ranks.table", json!({})).await);
    assert_eq!(r.character, "malakor");
    assert_eq!(r.ranks.len(), 6);
    assert_eq!(r.propose_min_rank, shadowrunr_types::Rank::Warlock);
    let lich = &r.ranks[4];
    assert_eq!((lich.title.as_str(), lich.weight), ("Lesser Lich", 2.2));
    assert_eq!(r.ranks[3].min_gifted, Some(5));
    assert!(r.ranks[0].tiers.is_empty());
    let v = rpc(&addr, "ranks.table", json!({ "character": "nobody" })).await;
    assert_eq!(v["error"]["code"], "not_found", "{v}");
}

#[tokio::test]
async fn every_contract_method_is_dispatched() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    for m in method_names() {
        let v = rpc(&addr, m, json!({})).await;
        assert_ne!(v["error"]["code"], "method_not_found", "{m}: {v}");
        let code = &v["error"]["code"];
        if !code.is_null() {
            let known = shadowrunr_types::ErrorCode::ALL
                .iter()
                .any(|c| code == c.as_str());
            assert!(known, "{m}: {v}");
        }
    }
}

/// The reconcile crate's real output round-trips through the contract's copies of its types.
#[tokio::test]
async fn reconcile_types_match_the_contract() {
    let tmp = tempfile::tempdir().unwrap();
    let addr = start(test_config(tmp.path(), repo().join("characters"))).await;
    fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(v: &Value) {
        let typed: T = serde_json::from_value(v["result"].clone()).unwrap();
        assert_eq!(serde_json::to_value(typed).unwrap(), v["result"]);
    }
    let d = rpc(
        &addr,
        "directive.add",
        json!({ "kind": "try", "text": "Open with a roll call", "scope": "SEG-01" }),
    )
    .await;
    round_trip::<Directive>(&d);
    let run = rpc(
        &addr,
        "reconcile.run",
        json!({ "source": { "tournament_id": "fixture-t" } }),
    )
    .await;
    let report_id = run["result"]["report_id"].as_str().unwrap().to_string();
    let report = rpc(&addr, "reconcile.report", json!({ "report_id": report_id })).await;
    round_trip::<ReconcileReport>(&report);
    let proposals = rpc(&addr, "plan.proposals", json!({ "report_id": report_id })).await;
    round_trip::<Vec<PlanOption>>(&proposals);
}
