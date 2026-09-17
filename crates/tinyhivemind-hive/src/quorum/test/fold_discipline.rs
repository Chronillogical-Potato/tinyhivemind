//! Fold discipline: `standings` must not depend on the order traces arrived
//! in or on how many times one was redelivered, and `carried` must read the
//! supporter count exactly as `QuorumPolicy::threshold` says.

use super::super::*;
use super::support::{deadlocked_transcript, policy, said};
use crate::trace::read;

#[test]
fn standings_are_order_independent() {
    let transcript = {
        let mut transcript = deadlocked_transcript();
        transcript.push(said(5, "critic", "!object >4 ^3"));
        transcript
    };
    let at = Sequence(5);
    let ordered = read(&transcript);

    let mut shuffled = ordered.clone();
    shuffled.reverse();
    let mut rotated = ordered.clone();
    rotated.rotate_left(2);

    let expected = standings(&ordered, at, &policy(2)).expect("folds");
    for permutation in [shuffled, rotated] {
        assert_eq!(
            standings(&permutation, at, &policy(2)).expect("folds"),
            expected,
            "a reordered fold must land in the same place, topic order included",
        );
    }
}

#[test]
fn standings_are_idempotent_over_duplicated_traces() {
    let transcript = deadlocked_transcript();
    let at = Sequence(4);
    let once = read(&transcript);
    let twice: Vec<_> = once.iter().cloned().chain(once.iter().cloned()).collect();
    assert_eq!(
        standings(&twice, at, &policy(2)).expect("folds"),
        standings(&once, at, &policy(2)).expect("folds"),
    );
}

#[test]
fn an_empty_medium_is_deliberating() {
    let standings = standings(&[], Sequence(0), &policy(2)).expect("folds");
    assert!(standings.is_empty());
    assert_eq!(
        consensus(&standings, &policy(2)),
        ConsensusState::Deliberating
    );
}

#[test]
fn carried_reports_whether_a_standing_reached_the_threshold() {
    let standing = TopicStanding {
        topic: "stage".into(),
        supporters: vec!["a".into(), "b".into()],
        silenced: Vec::new(),
        refuted_by: Vec::new(),
        support: 1,
        probability_support: 2 * u64::from(PROBABILITY_SCALE),
    };
    assert!(standing.carried(&policy(2)));
    assert!(!standing.carried(&policy(3)));
}
