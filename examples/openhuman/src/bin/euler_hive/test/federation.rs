//! Offline federations: asks, referrals, verification and synthesis.

use super::super::floors::Federation;
use super::support::*;

#[tokio::test]
async fn overlapping_questions_with_nested_asks_all_come_home_and_the_pipeline_finishes() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    // Two research seats ask theory at once; each theory answer first asks
    // compute, so two answering chains overlap on the theory desk.
    drive(&mut federation, |desk, seat, reason| match (desk, reason) {
        ("research", "opening") if seat != "research-scout" => refer("theory"),
        ("theory", "referral") => refer("compute"),
        _ => complete(&format!("{seat} done")),
    })
    .await;
    let research = &federation.floors[0];
    assert!(
        research.reported,
        "research never reported: a return was lost"
    );
    let finished = federation
        .finished
        .as_ref()
        .or(federation.undecided.as_ref())
        .expect("the pipeline reaches synthesis");
    assert_eq!(finished.desk, "synthesis");
    let returns = federation
        .referrals
        .iter()
        .filter(|frame| frame.kind == "return" && frame.to_desk == "research" && frame.accepted)
        .count();
    assert_eq!(returns, 2, "both research questions were answered");
}

#[tokio::test]
async fn a_refused_ask_gives_the_seat_its_turn_back_before_the_desk_reports() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    // research may not refer to verify: no declared link.
    drive(&mut federation, |desk, seat, reason| {
        match (desk, seat, reason) {
            ("research", "research-librarian", "opening") => refer("verify"),
            _ => complete(&format!("{seat} done")),
        }
    })
    .await;
    let refused = federation
        .referrals
        .iter()
        .find(|frame| frame.kind == "forward")
        .expect("the ask is recorded");
    assert!(!refused.accepted);
    let research = &federation.floors[0];
    assert!(research.reported);
    assert!(
        research
            .journal
            .iter()
            .any(|row| row.kind == "complete" && row.author == "research-librarian"),
        "the refused seat completed after its turn came back"
    );
    assert!(
        federation.undecided.is_some(),
        "the pipeline reached synthesis"
    );
}

#[test]
fn a_verify_seat_may_support_only_after_running_a_check_this_turn() {
    use super::super::tools::{CallRecord, grounded};
    let support = Some(super::super::tools::Verdict::Support);
    assert!(grounded("verify", support, &[]).is_err());
    let failed = [CallRecord {
        tool: "run_python".into(),
        summary: String::new(),
        ok: false,
    }];
    assert!(grounded("verify", support, &failed).is_err());
    let ran = [CallRecord {
        tool: "run_python".into(),
        summary: String::new(),
        ok: true,
    }];
    assert!(grounded("verify", support, &ran).is_ok());
    assert!(grounded("verify", Some(super::super::tools::Verdict::Object), &[]).is_ok());
    assert!(grounded("compute", support, &[]).is_ok());
}

#[tokio::test]
async fn a_synthesis_report_without_a_candidate_is_no_decision() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    drive(&mut federation, |_, seat, _| {
        complete(&format!("{seat} has no answer"))
    })
    .await;
    assert!(federation.finished.is_none(), "nothing was decided");
    assert!(
        federation.undecided.is_some(),
        "synthesis reported without a candidate"
    );
    assert_eq!(
        super::super::report::ending_of(false, true, None),
        "no_decision"
    );
    assert_eq!(
        super::super::report::ending_of(true, false, None),
        "converged"
    );
    assert_eq!(
        super::super::report::ending_of(false, false, Some("exhausted_turns")),
        "exhausted_turns"
    );
}

#[tokio::test]
async fn synthesis_commits_only_a_candidate_the_verify_desk_supported() {
    use super::super::tools::Verdict;
    let (_, hive) = lowered();
    let mut unverified = Federation::new(&hive).expect("federation builds");
    unverified.open("go");
    drive(&mut unverified, |desk, _, _| match desk {
        "synthesis" => propose("42", Some(Verdict::Support)),
        "verify" => propose("42", Some(Verdict::Object)),
        _ => complete("done"),
    })
    .await;
    assert!(
        unverified.finished.is_none(),
        "an unverified candidate is not committed"
    );
    assert!(unverified.undecided.is_some());

    let mut verified = Federation::new(&hive).expect("federation builds");
    verified.open("go");
    drive(&mut verified, |desk, _, _| match desk {
        "synthesis" | "verify" => propose("42", Some(Verdict::Support)),
        _ => complete("done"),
    })
    .await;
    let finished = verified
        .finished
        .as_ref()
        .expect("a verified candidate is committed");
    assert_eq!(finished.candidate.as_deref(), Some("42"));
}
