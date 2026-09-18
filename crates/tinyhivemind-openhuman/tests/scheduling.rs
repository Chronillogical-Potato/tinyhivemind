//! Public regressions for accepted routing order in completion scheduling.

#![allow(clippy::expect_used)]

use std::sync::{Mutex, OnceLock};

use openhuman_embed::{Agent, AgentSpec, Runtime, Workspace};
use tinyhivemind::{Conversation, Sequence, desk::Desk, responder::Probability, speech::Utterance};
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterFuture, RoutingEvaluation, RoutingPolicy, RoutingRequest,
};
use tinyhivemind_hive::{CompletionEpisodeState, ParticipantCompletion};
use tinyhivemind_openhuman::{
    AgentBinding, BroadcastRouting, CommittedUtterance, CompletionDriver, HiveGraph, OpenHumanHive,
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
                    .api_key("th_test_scheduling")
                    .build(),
            )
            .expect("OpenHuman runtime");
        let agents = [
            "runtime-one",
            "runtime-two",
            "runtime-three",
            "runtime-four",
        ]
        .into_iter()
        .map(|id| runtime.agent(AgentSpec::new(id)).expect("agent"))
        .collect();
        Fixture {
            executor: Mutex::new(executor),
            agents,
        }
    })
}

fn hive() -> OpenHumanHive {
    let ids = ["one", "two", "three", "four"];
    OpenHumanHive::new(
        HiveGraph::new(
            Desk {
                id: "engineering".into(),
                name: "Engineering".into(),
                description: None,
                members: ids.iter().map(|id| (*id).into()).collect(),
                responder_mode: tinyhivemind::desk::ResponderMode::Auto,
            },
            ids.iter()
                .map(|id| RouteCandidate {
                    id: (*id).into(),
                    label: (*id).into(),
                    role: None,
                    description: None,
                    capabilities: Vec::new(),
                    learned_topics: Vec::new(),
                    available: true,
                })
                .collect(),
        ),
        ids.iter()
            .enumerate()
            .map(|(index, id)| AgentBinding::new(*id, fixture().agents[index].clone()))
            .collect(),
    )
    .expect("hive")
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
        round_width: 4,
        choice_option_limit: 8,
    }
}

#[derive(Debug)]
struct OrderedRouter;

impl Router for OrderedRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
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

fn committed(author_id: &str, sequence: u64, utterance: Utterance) -> CommittedUtterance {
    CommittedUtterance {
        author_id: author_id.into(),
        sequence: Sequence(sequence),
        utterance,
    }
}

fn routing<'a>(router: &'a OrderedRouter, policy: &'a RoutingPolicy) -> BroadcastRouting<'a> {
    BroadcastRouting {
        primary: Some(router),
        reasoning: None,
        policy,
        roster_version: 1,
        thread_context: &[],
    }
}

#[test]
fn accepted_assignments_merge_in_order_without_duplicates() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let state = driver
        .start(CompletionEpisodeState {
            conversation: Conversation {
                desk_id: "engineering".into(),
                desk_name: "Engineering".into(),
                thread_root: None,
            },
            watermark: Sequence(0),
            participants: [
                ("one", None),
                ("two", Some(Sequence(0))),
                ("three", Some(Sequence(0))),
                ("four", None),
            ]
            .into_iter()
            .map(|(agent_id, completed_at)| ParticipantCompletion {
                agent_id: agent_id.into(),
                assigned_at: Sequence(0),
                completed_at,
            })
            .collect(),
        })
        .expect("state");
    let round = driver.pending_round(&state).expect("round");
    let router = OrderedRouter;
    let route_policy = policy();
    let transition = fixture()
        .block_on(driver.apply_committed_round(
            &state,
            &round,
            vec![
                committed(
                    "one",
                    1,
                    Utterance::Broadcast {
                        message: "first".into(),
                    },
                ),
                committed(
                    "four",
                    2,
                    Utterance::Broadcast {
                        message: "second".into(),
                    },
                ),
            ],
            Some(routing(&router, &route_policy)),
        ))
        .expect("round commits");

    assert_eq!(transition.actions.len(), 2);
    assert_eq!(
        driver
            .pending_round(&transition.state)
            .expect("next round")
            .agents()
            .iter()
            .map(|pending| pending.hive_agent_id)
            .collect::<Vec<_>>(),
        ["two", "three", "four", "one"],
    );
}

#[test]
fn completed_ids_are_pruned_from_accepted_order() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let initial = driver
        .start(
            CompletionEpisodeState::opened(
                Conversation {
                    desk_id: "engineering".into(),
                    desk_name: "Engineering".into(),
                    thread_root: None,
                },
                Sequence(0),
                ["one", "two", "three", "four"],
            )
            .expect("episode"),
        )
        .expect("state");
    let router = OrderedRouter;
    let route_policy = policy();
    let assigned = fixture()
        .block_on(driver.apply_committed(
            &initial,
            committed(
                "one",
                1,
                Utterance::Broadcast {
                    message: "schedule two first".into(),
                },
            ),
            Some(routing(&router, &route_policy)),
        ))
        .expect("broadcast")
        .state;
    let completed = fixture()
        .block_on(driver.apply_committed(
            &assigned,
            committed(
                "two",
                2,
                Utterance::CompleteEpisode {
                    message: "done".into(),
                },
            ),
            None,
        ))
        .expect("completion")
        .state;

    assert_eq!(
        driver
            .pending_round(&completed)
            .expect("next round")
            .agents()
            .iter()
            .map(|pending| pending.hive_agent_id)
            .collect::<Vec<_>>(),
        ["three", "four", "one"],
    );
}
