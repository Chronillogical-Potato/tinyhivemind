//! Pending-round validation and batch-preflight tests.

use super::*;

#[test]
fn committed_round_preflights_every_event_before_routing() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    let calls = Arc::new(AtomicUsize::new(0));
    let router = RecordingRouter {
        calls: Arc::clone(&calls),
        widths: Arc::new(Mutex::new(Vec::new())),
    };
    let route_policy = policy(2);
    let result = fixture().block_on(driver.apply_committed_round(
        &state,
        &round,
        vec![
            committed(
                "one",
                1,
                Utterance::Broadcast {
                    message: "route me".into(),
                },
            ),
            committed(
                "two",
                1,
                Utterance::Post {
                    message: "duplicate sequence".into(),
                },
            ),
        ],
        Some(BroadcastRouting {
            primary: Some(&router),
            reasoning: None,
            policy: &route_policy,
            roster_version: 1,
            thread_context: &[],
        }),
    ));
    assert!(matches!(
        result,
        Err(Error::DuplicateCommittedSequence {
            sequence: Sequence(1)
        })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.revision(), 0, "rejected batch cannot advance state");
}

#[test]
fn pending_round_rejects_stale_and_mismatched_state_snapshots() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let stale_round = driver.pending_round(&initial).expect("round");
    let advanced = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                1,
                Utterance::Post {
                    message: "advanced".into(),
                },
            ),
            None,
        ))
        .expect("commit")
        .state;
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &advanced,
            &stale_round,
            vec![
                committed(
                    "one",
                    2,
                    Utterance::Post {
                        message: "one".into()
                    }
                ),
                committed(
                    "two",
                    3,
                    Utterance::Post {
                        message: "two".into()
                    }
                ),
            ],
            None,
        )),
        Err(Error::StaleRound { .. })
    ));

    let other = driver.start(episode(&["two", "one"])).expect("other state");
    let other_round = driver.pending_round(&other).expect("other round");
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &initial,
            &other_round,
            vec![
                committed(
                    "two",
                    1,
                    Utterance::Post {
                        message: "two".into()
                    }
                ),
                committed(
                    "one",
                    2,
                    Utterance::Post {
                        message: "one".into()
                    }
                ),
            ],
            None,
        )),
        Err(Error::MismatchedRound { .. })
    ));
}

fn post(from: &str, at: u64) -> CommittedUtterance {
    committed(
        from,
        at,
        Utterance::Post {
            message: format!("{from} at {at}"),
        },
    )
}

#[test]
fn a_round_with_an_author_outside_it_is_refused_by_name() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    // Right count, wrong seat: `three` is on the desk but not in this round.
    let result = fixture().block_on(driver.apply_committed_round(
        &state,
        &round,
        vec![post("one", 1), post("three", 2)],
        None,
    ));
    assert!(matches!(
        result,
        Err(Error::UnexpectedRoundAuthor { agent_id }) if agent_id == "three"
    ));
}

#[test]
fn a_round_with_a_repeated_author_is_partial_rather_than_unexpected() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    // Right count, but one seat spoke twice and the other not at all.
    let result = fixture().block_on(driver.apply_committed_round(
        &state,
        &round,
        vec![post("one", 1), post("one", 2)],
        None,
    ));
    assert!(matches!(
        result,
        Err(Error::PartialRound {
            expected: 2,
            received: 1
        })
    ));
}

#[test]
fn an_empty_round_is_partial_and_never_a_replay() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    let result = fixture().block_on(driver.apply_committed_round(&state, &round, vec![], None));
    assert!(matches!(
        result,
        Err(Error::PartialRound {
            expected: 2,
            received: 0
        })
    ));
}

#[test]
fn an_ask_inside_a_round_is_preflighted_and_holds_the_asker() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    let transition = fixture()
        .block_on(driver.apply_committed_round(
            &state,
            &round,
            vec![
                committed(
                    "one",
                    1,
                    Utterance::Ask {
                        to: vec!["two".into()],
                        message: "which?".into(),
                    },
                ),
                post("two", 2),
            ],
            None,
        ))
        .expect("an ask needs no router");
    assert!(
        transition
            .actions
            .iter()
            .any(|action| matches!(action, HostAction::DeliverDm { .. })),
        "the ask is delivered to the seat it names"
    );
    assert!(
        transition.state.ledger().awaiting("one").is_some(),
        "the asker cannot complete until it is answered"
    );

    // Asking a seat that is not on the desk is refused before anything folds.
    let result = fixture().block_on(driver.apply_committed_round(
        &state,
        &round,
        vec![
            committed(
                "one",
                1,
                Utterance::Ask {
                    to: vec!["nobody".into()],
                    message: "?".into(),
                },
            ),
            post("two", 2),
        ],
        None,
    ));
    assert!(result.is_err(), "an unknown askee is a preflight error");
}
