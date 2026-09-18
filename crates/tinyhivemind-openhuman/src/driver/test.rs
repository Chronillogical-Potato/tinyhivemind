//! Completion-driver contract tests.

#![allow(clippy::expect_used)]

mod broadcast_fallback;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use tinyhivemind::{Conversation, Sequence, responder::Probability, speech::Utterance};
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterFuture, RoutingEvaluation, RoutingFallback, RoutingPlan, RoutingPolicy, RoutingRequest,
};
use tinyhivemind_hive::CompletionEpisodeState;

use super::{BroadcastRouting, CommittedUtterance, CompletionDriver, HostAction, route_ids};
use crate::{AgentBinding, Error, HiveGraph, OpenHumanHive, test_support::fixture};

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("test probability is bounded")
}

fn candidate(id: &str) -> RouteCandidate {
    RouteCandidate {
        id: id.into(),
        label: id.into(),
        role: None,
        description: None,
        capabilities: Vec::new(),
        learned_topics: Vec::new(),
        available: true,
    }
}

fn hive() -> OpenHumanHive {
    let ids = ["one", "two", "three", "four"];
    OpenHumanHive::new(
        HiveGraph::new(
            tinyhivemind::desk::Desk {
                id: "engineering".into(),
                name: "Engineering".into(),
                description: None,
                members: ids.iter().map(|id| (*id).into()).collect(),
                responder_mode: tinyhivemind::desk::ResponderMode::Auto,
            },
            ids.iter().map(|id| candidate(id)).collect(),
        ),
        ids.iter()
            .enumerate()
            .map(|(index, id)| AgentBinding::new(*id, fixture().agent(index)))
            .collect(),
    )
    .expect("fixture hive validates")
}

fn episode(participants: &[&str]) -> CompletionEpisodeState {
    CompletionEpisodeState::opened(
        Conversation {
            desk_id: "engineering".into(),
            desk_name: "Engineering".into(),
            thread_root: None,
        },
        Sequence(0),
        participants,
    )
    .expect("fixture episode opens")
}

fn committed(author_id: &str, sequence: u64, utterance: Utterance) -> CommittedUtterance {
    CommittedUtterance {
        author_id: author_id.into(),
        sequence: Sequence(sequence),
        utterance,
    }
}

#[derive(Debug)]
struct RecordingRouter {
    calls: Arc<AtomicUsize>,
    widths: Arc<Mutex<Vec<usize>>>,
}

impl Router for RecordingRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.widths
            .lock()
            .expect("width lock")
            .push(request.policy.round_width);
        let ids: Vec<_> = request
            .candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        let roster_version = request.roster_version;
        Box::pin(async move {
            Ok(RoutingEvaluation {
                primary_responder: ids[0].clone(),
                primary_probabilities: vec![
                    CandidateProbability {
                        candidate_id: ids[0].clone(),
                        probability: probability(400_000),
                    },
                    CandidateProbability {
                        candidate_id: ids[1].clone(),
                        probability: probability(250_000),
                    },
                    CandidateProbability {
                        candidate_id: ids[2].clone(),
                        probability: probability(250_000),
                    },
                    CandidateProbability {
                        candidate_id: "none".into(),
                        probability: probability(100_000),
                    },
                ],
                confidence: probability(900_000),
                needs_collaboration: probability(900_000),
                needs_clarification: probability(0),
                contributions: ids
                    .into_iter()
                    .map(|candidate_id| ContributionProbability {
                        candidate_id,
                        probability: probability(900_000),
                    })
                    .collect(),
                high_impact: probability(0),
                model_identity: "fixture".into(),
                question_schema_version: 1,
                roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

fn policy(round_width: usize) -> RoutingPolicy {
    RoutingPolicy {
        minimum_confidence: probability(600_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(700_000),
        high_impact_threshold: probability(700_000),
        round_width,
        choice_option_limit: 8,
    }
}

#[test]
fn deterministic_routes_preserve_the_canonical_id() {
    let plan = RoutingPlan::Fallback {
        responder_id: "solver".into(),
        reason: RoutingFallback::ExplicitMention,
    };
    assert_eq!(route_ids(&plan), ["solver"]);
}

#[test]
fn pending_work_is_bounded_and_state_advances_only_after_commits() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver
        .start(episode(&["one", "two", "three"]))
        .expect("state");
    let before = state.clone();
    let pending = driver.pending_round(&state).expect("pending round");
    assert_eq!(
        pending
            .agents()
            .iter()
            .map(|agent| agent.hive_agent_id)
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert_eq!(state, before, "reading pending work cannot advance state");

    let transition = fixture()
        .block_on(driver.apply_committed(
            &state,
            committed(
                "one",
                1,
                Utterance::CompleteEpisode {
                    message: "done".into(),
                },
            ),
            None,
        ))
        .expect("commit folds");
    assert_ne!(transition.state, state);
}

#[test]
fn payload_wire_forms_are_exact_and_all_fields_are_required() {
    let event = committed(
        "one",
        7,
        Utterance::Dm {
            to: vec!["two".into()],
            message: "private".into(),
        },
    );
    assert_eq!(
        serde_json::to_value(&event).expect("event serializes"),
        serde_json::json!({
            "author_id": "one",
            "sequence": 7,
            "utterance": {
                "kind": "dm",
                "to": ["two"],
                "message": "private"
            }
        })
    );
    for missing in ["author_id", "sequence", "utterance"] {
        let mut payload = serde_json::to_value(&event)
            .expect("event serializes")
            .as_object()
            .expect("event is an object")
            .clone();
        payload.remove(missing);
        assert!(
            serde_json::from_value::<CommittedUtterance>(payload.into()).is_err(),
            "missing {missing} must be rejected"
        );
    }

    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let state = fixture()
        .block_on(driver.apply_committed(&initial, event.clone(), None))
        .expect("event folds")
        .state;
    assert_eq!(
        serde_json::to_value(&state).expect("state serializes"),
        serde_json::json!({
            "episode": {
                "conversation": {
                    "desk_id": "engineering",
                    "desk_name": "Engineering",
                    "thread_root": null
                },
                "watermark": 0,
                "participants": [
                    {"agent_id": "one", "assigned_at": 0, "completed_at": null},
                    {"agent_id": "two", "assigned_at": 0, "completed_at": null}
                ]
            },
            "receipts": {
                "7": {
                    "event": {
                        "author_id": "one",
                        "sequence": 7,
                        "utterance": {
                            "kind": "dm",
                            "to": ["two"],
                            "message": "private"
                        }
                    }
                }
            },
            "freshness_floor": 7,
            "pending_order": [],
            "revision": 1
        })
    );
    for missing in [
        "episode",
        "receipts",
        "freshness_floor",
        "pending_order",
        "revision",
    ] {
        let mut payload = serde_json::to_value(&state)
            .expect("state serializes")
            .as_object()
            .expect("state is an object")
            .clone();
        payload.remove(missing);
        assert!(
            serde_json::from_value::<super::DriverState>(payload.into()).is_err(),
            "missing {missing} must be rejected"
        );
    }
}

#[test]
fn resumed_serialized_state_recognizes_every_replay_without_actions_or_rerouting() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let calls = Arc::new(AtomicUsize::new(0));
    let widths = Arc::new(Mutex::new(Vec::new()));
    let router = RecordingRouter {
        calls: Arc::clone(&calls),
        widths,
    };
    let route_policy = policy(4);
    let routing = BroadcastRouting {
        primary: Some(&router),
        reasoning: None,
        policy: &route_policy,
        roster_version: 7,
        thread_context: &[],
    };
    let events = vec![
        committed(
            "one",
            1,
            Utterance::Post {
                message: "visible".into(),
            },
        ),
        committed(
            "one",
            2,
            Utterance::Dm {
                to: vec!["two".into()],
                message: "private".into(),
            },
        ),
        committed(
            "one",
            3,
            Utterance::Broadcast {
                message: "help".into(),
            },
        ),
        committed(
            "one",
            4,
            Utterance::CompleteEpisode {
                message: "done".into(),
            },
        ),
    ];
    let mut state = driver
        .start(episode(&["one", "two", "three", "four"]))
        .expect("state");
    for event in &events {
        let transition = fixture()
            .block_on(driver.apply_committed(&state, event.clone(), Some(routing)))
            .expect("commit");
        state = transition.state;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let json = serde_json::to_string(&state).expect("driver state serializes");
    assert!(
        !json.contains("session"),
        "host session ids are not retained"
    );
    let restored = serde_json::from_str(&json).expect("driver state deserializes");
    let restored = driver.resume(restored).expect("serialized state resumes");
    for event in events {
        let replay = fixture()
            .block_on(driver.apply_committed(&restored, event, Some(routing)))
            .expect("exact replay");
        assert_eq!(replay.state, restored);
        assert!(
            replay.actions.is_empty(),
            "replay cannot emit a duplicate action"
        );
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "broadcast replay must not reroute"
    );
}

#[test]
fn broadcast_routing_is_clamped_to_the_driver_bound_in_deterministic_order() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let calls = Arc::new(AtomicUsize::new(0));
    let widths = Arc::new(Mutex::new(Vec::new()));
    let router = RecordingRouter {
        calls,
        widths: Arc::clone(&widths),
    };
    let route_policy = policy(4);
    let transition = fixture()
        .block_on(
            driver.apply_committed(
                &driver
                    .start(episode(&["one", "two", "three", "four"]))
                    .expect("state"),
                committed(
                    "one",
                    1,
                    Utterance::Broadcast {
                        message: "help".into(),
                    },
                ),
                Some(BroadcastRouting {
                    primary: Some(&router),
                    reasoning: None,
                    policy: &route_policy,
                    roster_version: 7,
                    thread_context: &[],
                }),
            ),
        )
        .expect("broadcast folds");
    assert_eq!(*widths.lock().expect("width lock"), [2]);
    assert!(matches!(
        transition.actions.as_slice(),
        [HostAction::RunAgents { .. }]
    ));
    if let [HostAction::RunAgents { agent_ids, plan }] = transition.actions.as_slice() {
        assert_eq!(agent_ids, &["two", "three"]);
        assert!(matches!(
            plan,
            RoutingPlan::Hive {
                primary_id,
                invited_ids,
                evaluation,
            } if primary_id == "two"
                && invited_ids == &["three"]
                && evaluation.roster_version == 7
        ));
    }
}

#[test]
fn rejects_stale_partial_duplicate_and_out_of_hive_commits() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let first = committed(
        "one",
        10,
        Utterance::Post {
            message: "newest".into(),
        },
    );
    let state = fixture()
        .block_on(driver.apply_committed(&state, first, None))
        .expect("first")
        .state;
    assert!(matches!(
        fixture().block_on(driver.apply_committed(
            &state,
            committed(
                "two",
                9,
                Utterance::Post {
                    message: "older".into()
                }
            ),
            None
        )),
        Err(Error::StaleCommittedEvent {
            sequence: Sequence(9)
        })
    ));
    assert!(matches!(
        fixture().block_on(driver.apply_committed(
            &state,
            committed(
                "two",
                10,
                Utterance::Post {
                    message: "conflict".into()
                }
            ),
            None
        )),
        Err(Error::DuplicateCommittedSequence {
            sequence: Sequence(10)
        })
    ));
    assert!(matches!(
        fixture().block_on(driver.apply_committed(
            &state,
            committed(
                "outsider",
                11,
                Utterance::Post {
                    message: "no".into()
                }
            ),
            None
        )),
        Err(Error::OutOfHiveParticipant { .. })
    ));

    let fresh = driver.start(episode(&["one", "two"])).expect("fresh");
    let round = driver.pending_round(&fresh).expect("round");
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &fresh,
            &round,
            vec![committed(
                "one",
                1,
                Utterance::Post {
                    message: "partial".into()
                }
            )],
            None,
        )),
        Err(Error::PartialRound {
            expected: 2,
            received: 1
        })
    ));
}

#[test]
fn committed_rounds_are_sequence_ordered_independent_of_input_order() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&initial).expect("round");
    let one = committed(
        "one",
        1,
        Utterance::Post {
            message: "one".into(),
        },
    );
    let two = committed(
        "two",
        2,
        Utterance::Post {
            message: "two".into(),
        },
    );
    let ascending = fixture()
        .block_on(driver.apply_committed_round(
            &initial,
            &round,
            vec![one.clone(), two.clone()],
            None,
        ))
        .expect("ascending");
    let descending = fixture()
        .block_on(driver.apply_committed_round(&initial, &round, vec![two, one], None))
        .expect("descending");
    assert_eq!(descending, ascending);

    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &initial,
            &round,
            vec![
                committed(
                    "one",
                    3,
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
        Err(Error::DuplicateCommittedSequence {
            sequence: Sequence(3)
        })
    ));
}

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
