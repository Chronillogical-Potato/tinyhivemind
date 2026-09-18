//! Behavioral coverage for persistence, routing, and batch preflight seams.

use super::*;

#[derive(Debug)]
struct ClarifyingRouter;

impl Router for ClarifyingRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        let roster_version = request.roster_version;
        let primary_probabilities = request
            .candidates
            .iter()
            .map(|candidate| CandidateProbability {
                candidate_id: candidate.id.clone(),
                probability: probability(0),
            })
            .chain(std::iter::once(CandidateProbability {
                candidate_id: "none".into(),
                probability: probability(1_000_000),
            }))
            .collect();
        let contributions = request
            .candidates
            .iter()
            .map(|candidate| ContributionProbability {
                candidate_id: candidate.id.clone(),
                probability: probability(0),
            })
            .collect();
        Box::pin(async move {
            Ok(RoutingEvaluation {
                primary_responder: "none".into(),
                primary_probabilities,
                confidence: probability(1_000_000),
                needs_collaboration: probability(0),
                needs_clarification: probability(1_000_000),
                contributions,
                high_impact: probability(0),
                model_identity: "clarifying-fixture".into(),
                question_schema_version: 1,
                roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

#[test]
fn resume_rejects_all_untrusted_episode_and_receipt_identities() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let base = driver.start(episode(&["one", "two"])).expect("state");

    let mut wrong_desk = base.clone();
    wrong_desk.episode.conversation.desk_id = "another".into();
    assert!(matches!(
        driver.resume(wrong_desk),
        Err(Error::OutOfHiveEpisode { .. })
    ));

    let mut no_participants = base.clone();
    no_participants.episode.participants.clear();
    assert!(matches!(
        driver.resume(no_participants),
        Err(Error::Completion(
            tinyhivemind_hive::Error::NoCompletionParticipants
        ))
    ));

    let mut blank_participant = base.clone();
    blank_participant.episode.participants[0].agent_id = "  ".into();
    assert!(matches!(
        driver.resume(blank_participant),
        Err(Error::Completion(
            tinyhivemind_hive::Error::InvalidCompletionParticipant
        ))
    ));

    let mut duplicate_participant = base.clone();
    duplicate_participant.episode.participants[1].agent_id = "one".into();
    assert!(matches!(
        driver.resume(duplicate_participant),
        Err(Error::Completion(
            tinyhivemind_hive::Error::DuplicateCompletionParticipant { .. }
        ))
    ));

    let mut outsider_participant = base.clone();
    outsider_participant.episode.participants[1].agent_id = "outsider".into();
    assert!(matches!(
        driver.resume(outsider_participant),
        Err(Error::OutOfHiveParticipant { .. })
    ));

    let committed_state = fixture()
        .block_on(driver.apply_committed(
            &base,
            committed(
                "one",
                5,
                Utterance::Post {
                    message: "persisted".into(),
                },
            ),
            None,
        ))
        .expect("post commits")
        .state;
    let mut stale_receipt = committed_state.clone();
    stale_receipt.episode.watermark = Sequence(5);
    assert!(matches!(
        driver.resume(stale_receipt),
        Err(Error::StaleCommittedEvent {
            sequence: Sequence(5)
        })
    ));

    let mut outsider_receipt = committed_state;
    outsider_receipt
        .receipts
        .get_mut(&Sequence(5))
        .expect("receipt")
        .event
        .author_id = "outsider".into();
    assert!(matches!(
        driver.resume(outsider_receipt),
        Err(Error::OutOfHiveParticipant { .. })
    ));
}

#[test]
fn pending_round_defends_against_unbound_replaced_state() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let mut state = driver.start(episode(&["one", "two"])).expect("state");
    state.episode.participants[0].agent_id = "outsider".into();

    assert!(matches!(
        driver.pending_round(&state),
        Err(Error::UnknownBoundAgent { agent_id }) if agent_id == "outsider"
    ));
}

#[test]
fn commits_private_completion_and_clarification_outcomes() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");

    let completed = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                1,
                Utterance::CompleteEpisode {
                    message: "finished".into(),
                },
            ),
            None,
        ))
        .expect("completion commits");
    assert_eq!(
        completed.state.episode.participants[0].completed_at,
        Some(Sequence(1))
    );

    let dm = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                2,
                Utterance::Dm {
                    to: vec!["two".into()],
                    message: "private".into(),
                },
            ),
            None,
        ))
        .expect("dm commits");
    assert!(matches!(
        dm.actions.as_slice(),
        [HostAction::DeliverDm { .. }]
    ));

    assert!(matches!(
        fixture().block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                3,
                Utterance::Broadcast {
                    message: "route this".into(),
                },
            ),
            None,
        )),
        Err(Error::MissingBroadcastRouting)
    ));

    let router = ClarifyingRouter;
    let route_policy = policy(2);
    let clarification = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                4,
                Utterance::Broadcast {
                    message: "need more context".into(),
                },
            ),
            Some(BroadcastRouting {
                primary: Some(&router),
                reasoning: Some(&router),
                policy: &route_policy,
                roster_version: 3,
                thread_context: &["prior question".into()],
            }),
        ))
        .expect("clarification commits");
    assert!(clarification.actions.is_empty());
    assert_eq!(clarification.state.revision(), 1);
}

#[test]
fn batch_preflight_accepts_a_prior_receipt_with_a_new_private_result() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let replayed = committed(
        "one",
        1,
        Utterance::Post {
            message: "already committed".into(),
        },
    );
    let state = fixture()
        .block_on(driver.apply_committed(&initial, replayed.clone(), None))
        .expect("post commits")
        .state;
    let round = driver.pending_round(&state).expect("round");
    let transition = fixture()
        .block_on(driver.apply_committed_round(
            &state,
            &round,
            vec![
                replayed.clone(),
                committed(
                    "two",
                    2,
                    Utterance::Dm {
                        to: vec!["one".into()],
                        message: "reply".into(),
                    },
                ),
            ],
            None,
        ))
        .expect("receipt and new dm batch commits");
    assert_eq!(transition.state.revision(), 2);
    assert!(matches!(
        transition.actions.as_slice(),
        [HostAction::DeliverDm { .. }]
    ));
}

#[test]
fn batch_preflight_rejects_conflicting_and_stale_new_events() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let state = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                1,
                Utterance::Post {
                    message: "already committed".into(),
                },
            ),
            None,
        ))
        .expect("post commits")
        .state;
    let round = driver.pending_round(&state).expect("round");
    let conflicting = committed(
        "one",
        1,
        Utterance::Post {
            message: "different content".into(),
        },
    );
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &state,
            &round,
            vec![
                conflicting,
                committed(
                    "two",
                    2,
                    Utterance::Post {
                        message: "new".into(),
                    },
                ),
            ],
            None,
        )),
        Err(Error::DuplicateCommittedSequence {
            sequence: Sequence(1)
        })
    ));

    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &state,
            &round,
            vec![
                committed(
                    "one",
                    0,
                    Utterance::Post {
                        message: "stale".into(),
                    },
                ),
                committed(
                    "two",
                    2,
                    Utterance::Post {
                        message: "new".into(),
                    },
                ),
            ],
            None,
        )),
        Err(Error::StaleCommittedEvent {
            sequence: Sequence(0)
        })
    ));
}

#[test]
fn batch_preflight_requires_routing_before_a_broadcast_can_commit() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let initial_round = driver.pending_round(&initial).expect("initial round");
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &initial,
            &initial_round,
            vec![
                committed(
                    "one",
                    1,
                    Utterance::Broadcast {
                        message: "needs router".into(),
                    },
                ),
                committed(
                    "two",
                    2,
                    Utterance::Post {
                        message: "second".into(),
                    },
                ),
            ],
            None,
        )),
        Err(Error::MissingBroadcastRouting)
    ));
}
