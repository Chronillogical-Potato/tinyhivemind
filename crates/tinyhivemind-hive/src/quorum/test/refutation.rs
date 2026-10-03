//! Refutation: the mechanism that caps a topic out of contention on cited
//! evidence, without touching the advocate cross-inhibition already silences.

use super::super::*;
use super::support::{contested_transcript, fold, policy, said, standing};
use crate::trace::read;

#[test]
fn refutations_are_recorded_but_cap_nothing_under_the_default_policy() {
    let transcript = [
        said(1, "planner", "!propose #stage Stage the rollout."),
        said(
            2,
            "critic",
            "!support #stage ^1 It bounds the blast radius.",
        ),
        said(3, "auditor", "!evidence The environment was retired."),
        said(4, "auditor", "!refute #stage ^3 Nowhere to stage it."),
        said(5, "scout", "!refute #stage ^3 Confirmed."),
    ];
    let default = QuorumPolicy {
        window: 100,
        ..QuorumPolicy::DEFAULT
    };
    let standings = fold(&transcript, &default);
    let held = standing(&standings, "stage");
    // The room's disagreement is on the record either way. Only the *effect*
    // is opt-in.
    assert_eq!(held.refuted_by, ["auditor", "scout"]);
    assert!(held.carried(&default));
}

#[test]
fn a_refutation_needs_both_a_topic_and_a_citation() {
    // The marker parses only with both qualifiers. Without either it deposits
    // nothing at all, rather than a trace that could cap a topic on nothing.
    let traces = read(&[
        said(1, "auditor", "!refute #stage ^0 Grounded and named."),
        said(2, "auditor", "!refute #stage Names a topic, cites nothing."),
        said(3, "auditor", "!refute ^1 Cites something, names no topic."),
        said(4, "auditor", "!refute Neither."),
    ]);
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].kind, TraceKind::Refute);
    assert_eq!(traces[0].sequence, Sequence(1));
    assert!(traces[0].grounded());
}

#[test]
fn refuters_below_the_cap_leave_a_carried_topic_carried() {
    let mut transcript = contested_transcript();
    transcript.push(said(4, "auditor", "!refute #stage ^3 Nowhere to stage it."));
    let standings = fold(&transcript, &policy(2));
    let held = standing(&standings, "stage");

    assert_eq!(held.refuted_by, ["auditor"]);
    // One refuter is below the default cap of two, so nothing is capped, and
    // no supporter was removed: refutation never silences anybody.
    assert_eq!(held.supporters, ["planner", "critic"]);
    assert!(held.carried(&policy(2)));
}

#[test]
fn repeated_refutation_by_one_member_counts_once() {
    let mut transcript = contested_transcript();
    transcript.push(said(4, "auditor", "!refute #stage ^3 Nowhere to stage it."));
    transcript.push(said(5, "auditor", "!refute #stage ^3 Still nowhere."));
    let held = fold(&transcript, &policy(2));
    let held = standing(&held, "stage");
    assert_eq!(held.refuted_by, ["auditor"]);
    assert!(held.carried(&policy(2)));
}

#[test]
fn refuting_a_topic_nobody_advocated_is_inert() {
    // A refutation attaches to a topic some member put on the floor. Otherwise
    // one member could manufacture a standing nobody else ever mentioned.
    let standings = fold(
        &[
            said(1, "auditor", "!evidence Nobody proposed this."),
            said(2, "auditor", "!refute #phantom ^1 Refuting thin air."),
        ],
        &policy(2),
    );
    assert!(standings.is_empty(), "{standings:?}");
}

#[test]
fn a_member_that_both_supports_and_refutes_a_topic_is_only_a_refuter() {
    let standings = fold(
        &[
            said(1, "planner", "!propose #stage Stage the rollout."),
            said(2, "critic", "!support #stage ^1 Agreed."),
            said(3, "critic", "!evidence The environment was retired."),
            said(4, "critic", "!refute #stage ^3 I was wrong about this."),
        ],
        &policy(2),
    );
    let held = standing(&standings, "stage");
    assert_eq!(held.supporters, ["planner"]);
    assert_eq!(held.refuted_by, ["critic"]);
    assert_eq!(held.support, importance(TraceKind::Propose));
}

#[test]
fn refutations_fold_commutatively_and_idempotently() {
    let mut transcript = contested_transcript();
    transcript.push(said(4, "auditor", "!refute #stage ^3 Nowhere to stage it."));
    transcript.push(said(5, "scout", "!refute #stage ^3 Confirmed."));
    let at = Sequence(5);

    let forward = read(&transcript);
    let mut reversed = forward.clone();
    reversed.reverse();
    let doubled: Vec<_> = forward
        .iter()
        .cloned()
        .chain(forward.iter().cloned())
        .collect();

    let expected = standings(&forward, at, &policy(2)).expect("folds");
    assert_eq!(
        standings(&reversed, at, &policy(2)).expect("folds"),
        expected
    );
    assert_eq!(
        standings(&doubled, at, &policy(2)).expect("folds"),
        expected
    );
}

#[test]
fn a_refutation_outside_the_window_is_not_recorded() {
    let mut transcript = contested_transcript();
    transcript.push(said(4, "auditor", "!refute #stage ^3 Nowhere to stage it."));
    transcript.push(said(5, "scout", "!refute #stage ^3 Confirmed."));
    transcript.push(said(40, "planner", "!propose #stage Raising it again."));
    transcript.push(said(41, "critic", "!support #stage ^40 Still worth it."));

    let narrow = QuorumPolicy {
        window: 5,
        ..policy(2)
    };
    let standings = standings(&read(&transcript), Sequence(41), &narrow).expect("folds");
    let held = standing(&standings, "stage");
    assert!(held.refuted_by.is_empty(), "{:?}", held.refuted_by);
    assert!(held.carried(&narrow));
}
