//! Public regressions for completion-driver replay and broadcast routing.

#![allow(clippy::expect_used)]

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicUsize, Ordering},
};

use openhuman_embed::{Agent, AgentSpec, Runtime, Workspace};
use tinyhivemind::{
    Conversation, Sequence,
    desk::{Desk, ResponderMode},
    responder::Probability,
    speech::Utterance,
};
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterFuture, RoutingEvaluation, RoutingPlan, RoutingPolicy, RoutingRequest,
};
use tinyhivemind_hive::{CompletionEpisodeState, apply_assignment, apply_completion};
use tinyhivemind_openhuman::{
    AgentBinding, BroadcastRouting, CommittedUtterance, CompletionDriver, Error, HiveGraph,
    HostAction, OpenHumanHive,
};

struct Fixture {
    executor: Mutex<tokio::runtime::Runtime>,
    agents: Vec<Agent>,
}

impl Fixture {
    fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.executor
            .lock()
            .expect("executor lock")
            .block_on(future)
    }
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let runtime = executor
            .block_on(
                Runtime::builder()
                    .workspace(Workspace::Ephemeral)
                    .api_key("th_test_review_regressions")
                    .build(),
            )
            .expect("OpenHuman runtime");
        let agents = ["runtime-one", "runtime-two", "runtime-three"]
            .into_iter()
            .map(|id| runtime.agent(AgentSpec::new(id)).expect("agent"))
            .collect();
        Fixture {
            executor: Mutex::new(executor),
            agents,
        }
    })
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
    let ids = ["one", "two", "three"];
    OpenHumanHive::new(
        HiveGraph::new(
            Desk {
                id: "engineering".into(),
                name: "Engineering".into(),
                description: None,
                members: ids.iter().map(|id| (*id).into()).collect(),
                responder_mode: ResponderMode::Auto,
            },
            ids.iter().map(|id| candidate(id)).collect(),
        ),
        ids.iter()
            .enumerate()
            .map(|(index, id)| AgentBinding::new(*id, fixture().agents[index].clone()))
            .collect(),
    )
    .expect("hive")
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
    .expect("episode")
}

fn committed(author_id: &str, sequence: u64, utterance: Utterance) -> CommittedUtterance {
    CommittedUtterance {
        author_id: author_id.into(),
        sequence: Sequence(sequence),
        utterance,
    }
}

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("bounded probability")
}

fn policy() -> RoutingPolicy {
    RoutingPolicy {
        minimum_confidence: probability(600_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(700_000),
        high_impact_threshold: probability(700_000),
        round_width: 2,
        choice_option_limit: 8,
    }
}

#[derive(Debug)]
struct CandidateRecordingRouter {
    calls: Arc<AtomicUsize>,
    candidates: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Router for CandidateRecordingRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let ids: Vec<_> = request
            .candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        self.candidates
            .lock()
            .expect("candidate lock")
            .push(ids.clone());
        let roster_version = request.roster_version;
        Box::pin(async move {
            let primary = ids.first().expect("router receives a candidate").clone();
            Ok(RoutingEvaluation {
                primary_responder: primary.clone(),
                primary_probabilities: vec![
                    CandidateProbability {
                        candidate_id: primary.clone(),
                        probability: probability(900_000),
                    },
                    CandidateProbability {
                        candidate_id: "none".into(),
                        probability: probability(100_000),
                    },
                ],
                confidence: probability(900_000),
                needs_collaboration: probability(0),
                needs_clarification: probability(0),
                contributions: vec![ContributionProbability {
                    candidate_id: primary,
                    probability: probability(900_000),
                }],
                high_impact: probability(0),
                model_identity: "fixture".into(),
                question_schema_version: 1,
                roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

#[test]
fn resumed_round_replay_is_a_noop_before_stale_round_validation() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver
        .start(episode(&["one", "two", "three"]))
        .expect("state");
    let round = driver.pending_round(&initial).expect("round");
    let calls = Arc::new(AtomicUsize::new(0));
    let router = CandidateRecordingRouter {
        calls: Arc::clone(&calls),
        candidates: Arc::new(Mutex::new(Vec::new())),
    };
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: Some(&router),
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let events = vec![
        committed(
            "one",
            1,
            Utterance::Broadcast {
                message: "help".into(),
            },
        ),
        committed(
            "two",
            2,
            Utterance::Post {
                message: "working".into(),
            },
        ),
    ];
    let committed_state = fixture()
        .block_on(driver.apply_committed_round(&initial, &round, events.clone(), Some(routing)))
        .expect("round commits")
        .state;
    let restored =
        serde_json::from_str(&serde_json::to_string(&committed_state).expect("serialize"))
            .expect("deserialize");
    let restored = driver.resume(restored).expect("resume");

    calls.store(0, Ordering::SeqCst);
    let replay = fixture()
        .block_on(driver.apply_committed_round(&restored, &round, events.clone(), Some(routing)))
        .expect("exact replay");
    assert_eq!(replay.state, restored);
    assert!(replay.actions.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let mut conflict = events.clone();
    conflict[0].utterance = Utterance::Post {
        message: "conflict".into(),
    };
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &restored,
            &round,
            conflict,
            Some(routing)
        )),
        Err(Error::StaleRound { .. })
    ));
    let mut new_event = events;
    new_event[0].sequence = Sequence(3);
    assert!(matches!(
        fixture().block_on(driver.apply_committed_round(
            &restored,
            &round,
            new_event,
            Some(routing)
        )),
        Err(Error::StaleRound { .. })
    ));
}

#[test]
fn completed_round_replays_after_restart_with_a_new_empty_pending_round() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let round = driver.pending_round(&initial).expect("pending round");
    let events = vec![
        committed(
            "one",
            1,
            Utterance::CompleteEpisode {
                message: "one done".into(),
            },
        ),
        committed(
            "two",
            2,
            Utterance::CompleteEpisode {
                message: "two done".into(),
            },
        ),
    ];
    let final_state = fixture()
        .block_on(driver.apply_committed_round(&initial, &round, events.clone(), None))
        .expect("round commits")
        .state;
    drop(round);

    let restored =
        serde_json::from_str(&serde_json::to_string(&final_state).expect("final state serializes"))
            .expect("final state deserializes");
    let restored = driver.resume(restored).expect("final state resumes");
    let empty_round = driver
        .pending_round(&restored)
        .expect("restored pending round");
    assert!(empty_round.is_empty());

    let replay = fixture()
        .block_on(driver.apply_committed_round(&restored, &empty_round, events, None))
        .expect("receipt-only batch replay");
    assert_eq!(replay.state, restored);
    assert!(replay.actions.is_empty());
}

#[test]
fn advanced_episode_sequences_are_the_freshness_floor_without_receipts() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let assigned = apply_assignment(&episode(&["one", "two"]), ["one"], Sequence(10))
        .expect("assignment advances episode state");
    let advanced = apply_completion(&assigned, "two", Sequence(12))
        .expect("completion advances episode state");
    let state = driver.start(advanced).expect("advanced state starts");

    for utterance in [
        Utterance::Post {
            message: "stale post".into(),
        },
        Utterance::Dm {
            to: vec!["two".into()],
            message: "stale dm".into(),
        },
    ] {
        assert!(matches!(
            fixture().block_on(driver.apply_committed(
                &state,
                committed("one", 9, utterance),
                None,
            )),
            Err(Error::StaleCommittedEvent {
                sequence: Sequence(9)
            })
        ));
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let router = CandidateRecordingRouter {
        calls: Arc::clone(&calls),
        candidates: Arc::new(Mutex::new(Vec::new())),
    };
    let route_policy = policy();
    assert!(matches!(
        fixture().block_on(driver.apply_committed(
            &state,
            committed(
                "one",
                11,
                Utterance::Broadcast {
                    message: "stale broadcast".into(),
                },
            ),
            Some(BroadcastRouting {
                primary: Some(&router),
                reasoning: None,
                policy: &route_policy,
                roster_version: 1,
                thread_context: &[],
            }),
        )),
        Err(Error::StaleCommittedEvent {
            sequence: Sequence(11)
        })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn resumed_state_rejects_inconsistent_revision_receipt_and_freshness_metadata() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let initial = driver.start(episode(&["one", "two"])).expect("state");
    let state = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                7,
                Utterance::Post {
                    message: "committed".into(),
                },
            ),
            None,
        ))
        .expect("event commits")
        .state;

    for (field, value, expected) in [
        (
            "revision",
            serde_json::json!(2),
            "driver state revision 2 does not match 1 receipts",
        ),
        (
            "freshness_floor",
            serde_json::json!(6),
            "driver freshness floor 6 does not match derived floor 7",
        ),
    ] {
        let mut payload = serde_json::to_value(&state).expect("state serializes");
        payload[field] = value;
        let decoded = serde_json::from_value(payload).expect("wire shape remains valid");
        let error = driver
            .resume(decoded)
            .expect_err("inconsistent state rejected");
        assert_eq!(error.to_string(), expected);
    }

    let mut payload = serde_json::to_value(&state).expect("state serializes");
    let receipt = payload["receipts"]
        .as_object_mut()
        .expect("receipts object")
        .remove("7")
        .expect("receipt at sequence seven");
    payload["receipts"]
        .as_object_mut()
        .expect("receipts object")
        .insert("8".into(), receipt);
    let decoded = serde_json::from_value(payload).expect("wire shape remains valid");
    assert!(matches!(
        driver.resume(decoded),
        Err(Error::InvalidReceiptSequence {
            stored: Sequence(8),
            event: Sequence(7)
        })
    ));
}

#[test]
fn subset_episode_broadcast_routes_only_to_participants() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 2).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let router = CandidateRecordingRouter {
        calls: Arc::clone(&calls),
        candidates: Arc::clone(&seen),
    };
    let route_policy = policy();
    let transition = fixture()
        .block_on(driver.apply_committed(
            &state,
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
                roster_version: 1,
                thread_context: &[],
            }),
        ))
        .expect("participant route applies");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        *seen.lock().expect("candidate lock"),
        [vec!["two".to_owned()]]
    );
    assert!(matches!(
        transition.actions.as_slice(),
        [HostAction::RunAgents { .. }]
    ));
    if let [HostAction::RunAgents { agent_ids, plan }] = transition.actions.as_slice() {
        assert_eq!(agent_ids, &["two"]);
        assert!(matches!(plan, RoutingPlan::One { responder_id, .. } if responder_id == "two"));
    }
    assert_eq!(transition.state.revision(), 1);
}
