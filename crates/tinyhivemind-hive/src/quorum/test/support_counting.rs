//! Plain support counting: who a topic's supporter set includes, what a
//! malformed policy rejects, and what a trace that names no topic or agent —
//! or is a deferral rather than a vote — contributes (nothing).

use super::super::*;
use super::support::{fold, policy, said, standing};
use tinyhivemind::aside::Audience;
use tinyhivemind::{SessionAuthor, SessionMessage};

#[test]
fn a_zero_threshold_or_window_is_rejected() {
    let zero_threshold = QuorumPolicy {
        threshold: 0,
        ..QuorumPolicy::DEFAULT
    };
    let error = standings(&[], Sequence(1), &zero_threshold).expect_err("zero threshold");
    assert_eq!(error.to_string(), "quorum threshold must not be zero");

    let zero_window = QuorumPolicy {
        window: 0,
        ..QuorumPolicy::DEFAULT
    };
    let error = standings(&[], Sequence(1), &zero_window).expect_err("zero window");
    assert_eq!(error.to_string(), "quorum window must not be zero");

    let zero_cap = QuorumPolicy {
        refutation_cap: Some(0),
        ..QuorumPolicy::DEFAULT
    };
    let error = standings(&[], Sequence(1), &zero_cap).expect_err("zero refutation cap");
    assert_eq!(error.to_string(), "refutation cap must not be zero");
}

#[test]
fn a_proposer_supports_its_own_topic() {
    let standings = fold(&[said(1, "planner", "!propose #stage")], &policy(2));
    assert_eq!(standing(&standings, "stage").supporters, ["planner"]);
}

#[test]
fn distinct_supporters_carry_a_topic_and_repeat_support_does_not() {
    let repeated = fold(
        &[
            said(1, "planner", "!propose #stage"),
            said(2, "planner", "!support #stage ^1"),
            said(3, "planner", "!support #stage ^1"),
        ],
        &policy(2),
    );
    assert_eq!(standing(&repeated, "stage").supporters, ["planner"]);
    assert_eq!(
        consensus(&repeated, &policy(2)),
        ConsensusState::Deliberating
    );

    let distinct = fold(
        &[
            said(1, "planner", "!propose #stage"),
            said(2, "critic", "!support #stage ^1"),
        ],
        &policy(2),
    );
    assert_eq!(
        consensus(&distinct, &policy(2)),
        ConsensusState::Quorum {
            topic: "stage".into()
        },
    );
}

#[test]
fn an_ungrounded_support_moves_neither_the_supporters_nor_the_weight() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "critic", "!support #stage"),
    ];
    let grounded_required = fold(&transcript, &policy(2));
    let held = standing(&grounded_required, "stage");
    assert_eq!(held.supporters, ["planner"]);
    assert_eq!(held.support, importance(TraceKind::Propose));

    // The same input under a policy that does not require grounds carries.
    let lax = QuorumPolicy {
        require_grounded: false,
        ..policy(2)
    };
    let permissive = fold(&transcript, &lax);
    assert_eq!(
        standing(&permissive, "stage").supporters,
        ["planner", "critic"]
    );
}

#[test]
fn support_outside_the_window_stops_counting() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(50, "critic", "!support #stage ^1"),
    ];
    let narrow = QuorumPolicy {
        window: 10,
        ..policy(2)
    };
    let standings = fold(&transcript, &narrow);
    assert_eq!(standing(&standings, "stage").supporters, ["critic"]);
    assert_eq!(consensus(&standings, &narrow), ConsensusState::Deliberating);
}

#[test]
fn a_trace_without_a_topic_or_an_agent_author_is_not_counted() {
    let transcript = [
        said(1, "planner", "!propose #stage"),
        said(2, "critic", "!support ^1"),
        SessionMessage {
            sequence: Sequence(3),
            author: SessionAuthor::Operator,
            content: "!support #stage ^1".into(),
            audience: Audience::Desk,
            elided: None,
        },
    ];
    let standings = fold(&transcript, &policy(2));
    assert_eq!(standing(&standings, "stage").supporters, ["planner"]);
}

#[test]
fn a_deferral_moves_no_support_and_creates_no_standing() {
    let transcript = [
        said(1, "planner", "!propose #stage Stage the rollout."),
        said(2, "critic", "!support #stage ^1 Bounds the blast radius."),
        // Abstention is not a vote in either direction, and it must not be
        // able to open a standing for a topic nobody advocated.
        said(3, "scout", "!defer #stage ^1 Not my area."),
        said(4, "scout", "!defer #pool The archivist measured this."),
    ];
    let folded = fold(&transcript, &policy(2));
    assert_eq!(folded.len(), 1);
    let stage = standing(&folded, "stage");
    assert_eq!(stage.supporters, ["planner", "critic"]);
    assert!(stage.silenced.is_empty(), "{:?}", stage.silenced);
    assert!(stage.refuted_by.is_empty(), "{:?}", stage.refuted_by);

    // The same room without the two deferrals folds identically.
    let without = fold(&transcript[..2], &policy(2));
    assert_eq!(folded, without);
}
