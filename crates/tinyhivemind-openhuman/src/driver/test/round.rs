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
