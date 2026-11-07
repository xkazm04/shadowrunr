//! A case move is never aired without its explanation (C2): for each case op (dispatch, reveal,
//! pin, show), the op and the spoken line explaining it are one delivery unit through the pace
//! trim, the lag ceiling and the output gate. A move whose line did not air is not emitted and
//! is carried to the next eligible beat, at most `CASE_CARRY_MAX` times, then skipped. Scripted
//! `FakeEngine`, no quota.

use shadowrunr_types::{EvidenceGrade, Op};

use super::*;
use crate::orchestrate::case::{
    CASE_CARRY_MAX, CASE_DEFERRED, CASE_DELIVERY_RULE, CASE_SKIPPED, CASE_UNEXPLAINED,
};
use crate::pace::{LAGGED, PACE_RULE, TRIMMED};

/// One case op under test: the segment context whose case loop asks for it, the op line the
/// reply carries, and a line naming its card or record.
struct CaseOp {
    name: &'static str,
    ctx: SegmentContext,
    op: &'static str,
    explains: &'static str,
}

fn card(id: &str, label: &str, fog: bool, grade: Option<EvidenceGrade>) -> shadowrunr_types::Hex {
    let mut h = hex(id, label, grade);
    h.fog = fog;
    h
}

fn case_ctx(op: &str, summary: &str, hexes: Vec<shadowrunr_types::Hex>) -> SegmentContext {
    let mut c = ctx(&[op]);
    c.segment = SegmentId::Seg02aHexmap;
    c.state = "investigating".into();
    c.state_summary = summary.into();
    c.hexes = hexes;
    c
}

fn case_ops() -> Vec<CaseOp> {
    vec![
        CaseOp {
            name: "dispatch",
            ctx: case_ctx(
                "dispatch_research",
                "No poll open.",
                vec![
                    card("HEX-01", "Glimmerquay Yield Syndicate", false, None),
                    card("HEX-02", "Ashpetal Holdings Ltd", true, None),
                ],
            ),
            op: r#"OP: {"op":"dispatch_research","job_id":"case-1","brief":"HEX-02: what the records say"}"#,
            explains: "The bone-clerks ride for Ashpetal Holdings, quills sharpened for paperwork.",
        },
        CaseOp {
            name: "reveal",
            ctx: case_ctx(
                "reveal_hex",
                "case file (planted FICTIONAL demo records): HEX-02 REP-01 REP: Ashpetal \
                 registry extract (fictional)",
                vec![
                    card("HEX-01", "Glimmerquay Yield Syndicate", false, None),
                    card("HEX-02", "Ashpetal Holdings Ltd", true, None),
                ],
            ),
            op: r#"OP: {"op":"reveal_hex","hex":"HEX-02","grade":"REP"}"#,
            explains: "Ashpetal Holdings turns face up, allegedly, its paperwork reeking of ink.",
        },
        CaseOp {
            name: "pin",
            ctx: case_ctx(
                "pin_link",
                "case file (planted FICTIONAL demo records): HEX-04 CLM-04 CLM: community \
                 trace\nBoard: 2 cards face up, 0 strings. Open leads: HEX-04 (wallet) has a \
                 returned record and no string.",
                vec![
                    card(
                        "HEX-04",
                        "Treasury wallet demo-0001",
                        false,
                        Some(EvidenceGrade::Clm),
                    ),
                    card(
                        "HEX-09",
                        "Mixer hop wallet demo-0002",
                        false,
                        Some(EvidenceGrade::Clm),
                    ),
                ],
            ),
            op: r#"OP: {"op":"pin_link","from":"HEX-04","to":"HEX-09","rel":"paid","record_id":"CLM-04"}"#,
            explains: "A string from the Treasury wallet to the Mixer hop wallet, allegedly paid.",
        },
        CaseOp {
            name: "show",
            ctx: case_ctx(
                "show_screen",
                "case file (planted FICTIONAL demo records): HEX-02 DOC-01 DOC: Ashpetal \
                 dissolution notice (fictional)",
                vec![card(
                    "HEX-02",
                    "Ashpetal Holdings Ltd",
                    false,
                    Some(EvidenceGrade::Doc),
                )],
            ),
            op: r#"OP: {"op":"show_screen","screen":"browser","record_id":"DOC-01"}"#,
            explains: "Up on the browser: the Ashpetal dissolution notice, stamped twice for luck.",
        },
    ]
}

/// The case ops a beat emitted.
fn case_op_events(ev: &[StreamEvent]) -> Vec<Op> {
    ev.iter()
        .filter_map(|e| match e {
            StreamEvent::Op { op, .. } if crate::orchestrate::case::is_case_op(op) => {
                Some(op.clone())
            }
            _ => None,
        })
        .collect()
}

/// The `case_delivery` records of a beat.
fn deliveries(ev: &[StreamEvent]) -> Vec<String> {
    mods(ev, ModStage::Output)
        .into_iter()
        .filter(|(r, _, _)| r == CASE_DELIVERY_RULE)
        .map(|(_, _, a)| a)
        .collect()
}

fn plan_reason(ev: &[StreamEvent]) -> String {
    ev.iter()
        .find_map(|e| match e {
            StreamEvent::BeatPlan { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// A spoken line of exactly `n` characters, distinct per `tag`.
fn filler(tag: &str, n: usize) -> String {
    let mut s = format!("The {tag} ledger");
    let mut k = 0;
    while s.chars().count() < n {
        k += 1;
        s.push_str(&format!(" {tag}{k}"));
    }
    s.truncate(n - 1);
    s.push('.');
    s
}

/// No speech hold and a generous budget; `lag_ceiling_ms` as given.
fn tuned(lag_ceiling_ms: i64, budget_chars: usize) -> BrainTuning {
    BrainTuning {
        pace: PaceConfig {
            speech_lead_ms: i64::MAX,
            budget_chars,
            lag_ceiling_ms,
            ..PaceConfig::default()
        },
        ..tuning()
    }
}

fn asker() -> Viewer {
    viewer("b1", "Bob", Rank::Cultist)
}

fn ask(id: &str, ts: i64) -> ChatMsg {
    msg(
        id,
        ts,
        &asker(),
        "@Malakor is that wallet card a scheme or what?",
    )
}

/// Every emitted case op has its explaining line aired in the same beat.
fn assert_explained(ev: &[StreamEvent], explains: &str, what: &str) {
    if !case_op_events(ev).is_empty() {
        assert!(
            says(ev).iter().any(|s| s == explains),
            "{what}: a case op aired without its line: {:?}",
            says(ev)
        );
    }
}

#[tokio::test]
async fn a_case_move_with_its_line_airs_on_a_normal_beat() {
    for c in case_ops() {
        let dir = tempfile::tempdir().unwrap();
        let engine = FakeEngine::new();
        let mut b = brain_with(
            dir.path(),
            Arc::new(engine.clone()),
            c.name,
            tuned(13_000, 400),
        )
        .await;
        engine.push(Role::CoreThread, Ok(&format!("{}\n{}", c.explains, c.op)));
        let ev = b.tick(1_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(
            plan_reason(&ev).starts_with("case: "),
            "{}: {}",
            c.name,
            plan_reason(&ev)
        );
        assert_eq!(case_op_events(&ev).len(), 1, "{}", c.name);
        assert_explained(&ev, c.explains, c.name);
        assert!(deliveries(&ev).is_empty(), "{}", c.name);
    }
}

/// The trim keeps the answer and the case line together, for every case op (the pin's hold,
/// generalised), and drops only the lines past them.
#[tokio::test]
async fn a_trimmed_case_beat_with_chat_keeps_the_line_and_airs_the_op() {
    for c in case_ops() {
        let dir = tempfile::tempdir().unwrap();
        let engine = FakeEngine::new();
        let mut b = brain_with(
            dir.path(),
            Arc::new(engine.clone()),
            c.name,
            tuned(i64::MAX, 150),
        )
        .await;
        let answer = filler("answer", 100);
        let extra = filler("extra", 60);
        engine.push(
            Role::CoreThread,
            Ok(&format!("{answer}\n{}\n{}\n{extra}", c.explains, c.op)),
        );
        let ev = b.tick(1_000, vec![ask("t1", 900)], &c.ctx).await.unwrap();
        assert!(
            plan_reason(&ev).contains("with chat"),
            "{}: {}",
            c.name,
            plan_reason(&ev)
        );
        assert_eq!(
            says(&ev),
            [answer.clone(), c.explains.to_string()],
            "{}",
            c.name
        );
        assert_eq!(
            mods(&ev, ModStage::Output),
            vec![(PACE_RULE.into(), None, TRIMMED.into())],
            "{}",
            c.name
        );
        assert_eq!(case_op_events(&ev).len(), 1, "{}", c.name);
        assert_explained(&ev, c.explains, c.name);
    }
}

/// The lag ceiling drops the case line behind the chat answer: the op is held back, the move
/// carried, and once there is room it airs exactly once, with its line.
#[tokio::test]
async fn a_lagged_case_line_defers_the_op_and_it_airs_once_when_there_is_room() {
    for c in case_ops() {
        let dir = tempfile::tempdir().unwrap();
        let engine = FakeEngine::new();
        // A 3 s ceiling: the second line of a beat (behind a 100-character answer) is late.
        let mut b = brain_with(
            dir.path(),
            Arc::new(engine.clone()),
            c.name,
            tuned(3_000, 400),
        )
        .await;
        let answer = filler("answer", 100);
        engine.push(
            Role::CoreThread,
            Ok(&format!("{answer}\n{}\n{}", c.explains, c.op)),
        );
        let ev1 = b.tick(1_000, vec![ask("t1", 900)], &c.ctx).await.unwrap();
        assert_eq!(
            says(&ev1),
            std::slice::from_ref(&answer),
            "{}: chat keeps priority",
            c.name
        );
        assert!(
            mods(&ev1, ModStage::Output).contains(&(PACE_RULE.into(), None, LAGGED.into())),
            "{}",
            c.name
        );
        assert!(case_op_events(&ev1).is_empty(), "{}: op held back", c.name);
        assert_eq!(deliveries(&ev1), [CASE_DEFERRED], "{}", c.name);

        // The next beat carries the move again (no case clock to wait out), alone: it airs.
        engine.push(Role::CoreThread, Ok(&format!("{}\n{}", c.explains, c.op)));
        let ev2 = b.tick(6_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(
            plan_reason(&ev2).starts_with("case: "),
            "{}: {}",
            c.name,
            plan_reason(&ev2)
        );
        assert!(
            last_core_prompt(&engine).contains("Advance the case this beat"),
            "{}",
            c.name
        );
        assert_eq!(case_op_events(&ev2).len(), 1, "{}", c.name);
        assert_explained(&ev2, c.explains, c.name);
        assert!(deliveries(&ev2).is_empty(), "{}", c.name);

        // Delivered: the move is settled and not asked again.
        engine.push(Role::CoreThread, Ok("The crypt hums along."));
        let ev3 = b.tick(11_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(
            !plan_reason(&ev3).contains(&format!("case: {}", c.name)),
            "{}: {}",
            c.name,
            plan_reason(&ev3)
        );
        let total = [&ev1, &ev2, &ev3]
            .iter()
            .map(|e| case_op_events(e).len())
            .sum::<usize>();
        assert_eq!(
            total, 1,
            "{}: the deferred op executes exactly once",
            c.name
        );
    }
}

/// The output gate withholds the case line (the repetition guard: it aired a beat ago): the op is
/// held back with it, and airs once a later beat says a fresh line.
#[tokio::test]
async fn a_case_line_the_gate_withholds_takes_the_op_with_it() {
    for c in case_ops() {
        let dir = tempfile::tempdir().unwrap();
        let engine = FakeEngine::new();
        let mut b = brain_with(
            dir.path(),
            Arc::new(engine.clone()),
            c.name,
            tuned(13_000, 400),
        )
        .await;
        // A plain beat says the line first, outside the case.
        engine.push(Role::CoreThread, Ok(c.explains));
        let ev0 = b.tick(1_000, Vec::new(), &ctx(&[])).await.unwrap();
        assert_eq!(says(&ev0), [c.explains.to_string()], "{}", c.name);
        // The case beat repeats it: the guard takes the line, and the op goes with it.
        let opener = filler("opener", 60);
        engine.push(
            Role::CoreThread,
            Ok(&format!("{}\n{}\n{opener}", c.explains, c.op)),
        );
        let ev1 = b.tick(20_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(plan_reason(&ev1).starts_with("case: "), "{}", c.name);
        assert!(!says(&ev1).iter().any(|s| s == c.explains), "{}", c.name);
        assert!(case_op_events(&ev1).is_empty(), "{}: op held back", c.name);
        assert_eq!(deliveries(&ev1), [CASE_DEFERRED], "{}", c.name);
        // A fresh line carries it.
        let fresh = filler("fresh", 80);
        engine.push(Role::CoreThread, Ok(&format!("{fresh}\n{}", c.op)));
        let ev2 = b.tick(40_000, Vec::new(), &c.ctx).await.unwrap();
        assert_eq!(case_op_events(&ev2).len(), 1, "{}", c.name);
        assert!(says(&ev2).contains(&fresh), "{}", c.name);
    }
}

/// A failed core call rolls the case ask back: the next beat asks again and the op airs once.
#[tokio::test]
async fn a_core_failure_on_a_case_beat_airs_the_op_once_on_the_next() {
    for c in case_ops() {
        let dir = tempfile::tempdir().unwrap();
        let engine = FakeEngine::new();
        let mut b = brain_with(
            dir.path(),
            Arc::new(engine.clone()),
            c.name,
            tuned(13_000, 400),
        )
        .await;
        engine.push(
            Role::CoreThread,
            Err(EngineError::Timeout { after_ms: 30_000 }),
        );
        let ev1 = b.tick(1_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(case_op_events(&ev1).is_empty(), "{}", c.name);
        engine.push(Role::CoreThread, Ok(&format!("{}\n{}", c.explains, c.op)));
        let ev2 = b.tick(6_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(plan_reason(&ev2).starts_with("case: "), "{}", c.name);
        assert_eq!(case_op_events(&ev2).len(), 1, "{}", c.name);
        assert_explained(&ev2, c.explains, c.name);
        engine.push(Role::CoreThread, Ok("Quiet in the archive."));
        let ev3 = b.tick(11_000, Vec::new(), &c.ctx).await.unwrap();
        assert!(case_op_events(&ev3).is_empty(), "{}", c.name);
    }
}

/// A move whose line never airs is carried `CASE_CARRY_MAX` times, then skipped with a record,
/// and not asked for again on the next beat.
#[tokio::test]
async fn a_move_carried_past_its_retries_is_skipped_with_a_record() {
    let c = case_ops().remove(0);
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(
        dir.path(),
        Arc::new(engine.clone()),
        "skip",
        tuned(3_000, 400),
    )
    .await;
    let mut seen = Vec::new();
    let mut t = 1_000;
    for k in 0..=CASE_CARRY_MAX {
        let answer = filler(&format!("answer{k}"), 100);
        engine.push(
            Role::CoreThread,
            Ok(&format!("{answer}\n{}\n{}", c.explains, c.op)),
        );
        let ev = b
            .tick(t, vec![ask(&format!("t{k}"), t - 100)], &c.ctx)
            .await
            .unwrap();
        assert!(
            plan_reason(&ev).starts_with("case: "),
            "{}",
            plan_reason(&ev)
        );
        assert!(case_op_events(&ev).is_empty());
        seen.extend(deliveries(&ev));
        t += 5_000;
    }
    let mut want = vec![CASE_DEFERRED.to_string(); CASE_CARRY_MAX as usize];
    want.push(CASE_SKIPPED.into());
    assert_eq!(seen, want);
    engine.push(Role::CoreThread, Ok("The ledger rests."));
    let ev = b.tick(t, Vec::new(), &c.ctx).await.unwrap();
    assert!(
        !plan_reason(&ev).starts_with("case: "),
        "{}",
        plan_reason(&ev)
    );
}

/// A case op with no host line at all (an op-only reply) does not air, and one the model made
/// unasked is dropped with its record.
#[tokio::test]
async fn an_op_with_no_line_does_not_air() {
    let c = case_ops().remove(3);
    let dir = tempfile::tempdir().unwrap();
    let engine = FakeEngine::new();
    let mut b = brain_with(
        dir.path(),
        Arc::new(engine.clone()),
        "bare",
        tuned(13_000, 400),
    )
    .await;
    engine.push(Role::CoreThread, Ok(c.op));
    let ev = b.tick(1_000, Vec::new(), &c.ctx).await.unwrap();
    assert!(case_op_events(&ev).is_empty());
    assert_eq!(deliveries(&ev), [CASE_DEFERRED]);
    // Unasked: a beat outside the case segment pins nothing on its own without a line.
    let dir = tempfile::tempdir().unwrap();
    let mut b = brain_with(
        dir.path(),
        Arc::new(engine.clone()),
        "bare2",
        tuned(13_000, 400),
    )
    .await;
    engine.push(Role::CoreThread, Ok(c.op));
    let ev = b
        .tick(1_000, Vec::new(), &ctx(&["show_screen"]))
        .await
        .unwrap();
    assert!(case_op_events(&ev).is_empty());
    assert_eq!(deliveries(&ev), [CASE_UNEXPLAINED]);
}
