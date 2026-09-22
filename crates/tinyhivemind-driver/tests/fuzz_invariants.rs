//! Properties that must hold whatever order events arrive in.
//!
//! The driver's job is to keep invariants the folds cannot check for
//! themselves, under events a durable medium may reorder or redeliver. The
//! defects a benchmark found the first time a loop like this ran were of that
//! shape -- a sequence that could never be reached, a participant nothing would
//! ever wake -- so these are asserted over random operation sequences rather
//! than over the cases someone thought to write down.
//!
//! An operation that is *refused* is a pass. Corruption is the failure.

#![allow(clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use tinyhivemind::{Conversation, Sequence, desk::Desk, responder::Probability, speech::Utterance};
use tinyhivemind_driver::{
    AgentBinding, BoundAgent, BoundHive, BroadcastRouting, CommittedUtterance, CompletionDriver,
    DriverState, HiveGraph,
};
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterFuture, RoutingEvaluation, RoutingPolicy, RoutingRequest,
};
use tinyhivemind_hive::CompletionEpisodeState;

const IDS: [&str; 4] = ["a0", "a1", "a2", "a3"];

/// A seat with nothing behind it: the driver stores a handle and hands it
/// back, so a name is all a test needs.
#[derive(Clone, Debug)]
struct Seat(String);

impl BoundAgent for Seat {
    fn runtime_id(&self) -> &str {
        &self.0
    }
}

struct Fixture {
    executor: Mutex<tokio::runtime::Runtime>,
    agents: Vec<Seat>,
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
    FIXTURE.get_or_init(|| Fixture {
        executor: Mutex::new(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("executor"),
        ),
        agents: IDS.into_iter().map(|id| Seat(id.to_string())).collect(),
    })
}

/// Deterministic and dependency-free, so a failure is reproducible from a seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap_or(0)
    }
}

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("bounded")
}

/// Routes each broadcast to a different candidate in turn, alone.
#[derive(Debug, Default)]
struct CyclingRouter {
    calls: AtomicUsize,
}

impl Router for CyclingRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        let eligible: Vec<String> = request
            .candidates
            .iter()
            .filter(|candidate| candidate.available)
            .map(|candidate| candidate.id.clone())
            .collect();
        let pick = self.calls.fetch_add(1, Ordering::SeqCst) % eligible.len().max(1);
        let roster_version = request.roster_version;
        Box::pin(async move {
            // A valid domain: every eligible candidate plus `none`, summing
            // to the scale, one contribution each, the pick on top.
            let others = u32::try_from(eligible.len().saturating_sub(1)).expect("small");
            let mut primary_probabilities: Vec<CandidateProbability> = eligible
                .iter()
                .enumerate()
                .map(|(index, id)| CandidateProbability {
                    candidate_id: id.clone(),
                    probability: probability(if index == pick { 600_000 } else { 100_000 }),
                })
                .collect();
            primary_probabilities.push(CandidateProbability {
                candidate_id: "none".into(),
                probability: probability(400_000 - 100_000 * others),
            });
            Ok(RoutingEvaluation {
                primary_responder: eligible[pick].clone(),
                primary_probabilities,
                confidence: probability(900_000),
                needs_collaboration: probability(0),
                needs_clarification: probability(0),
                contributions: eligible
                    .iter()
                    .map(|id| ContributionProbability {
                        candidate_id: id.clone(),
                        probability: probability(500_000),
                    })
                    .collect(),
                high_impact: probability(0),
                model_identity: "cycling".into(),
                question_schema_version: 1,
                roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

fn hive(count: usize) -> BoundHive<Seat> {
    let ids = &IDS[..count];
    BoundHive::new(
        HiveGraph::new(
            Desk {
                id: "desk".into(),
                name: "Desk".into(),
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

fn opened(count: usize) -> CompletionEpisodeState {
    CompletionEpisodeState::opened(
        Conversation {
            desk_id: "desk".into(),
            desk_name: "Desk".into(),
            thread_root: None,
        },
        Sequence(1),
        &IDS[..count],
    )
    .expect("episode")
}

/// Everything that must be true of a driver state after *any* operation.
fn hold(driver: &CompletionDriver<'_, Seat>, state: &DriverState, settled_before: usize) {
    let pending: Vec<&str> = state
        .episode()
        .participants
        .iter()
        .filter(|participant| participant.is_pending())
        .map(|participant| participant.agent_id.as_str())
        .collect();
    for participant in &state.episode().participants {
        let open: Vec<_> = participant
            .assignments
            .iter()
            .enumerate()
            .filter(|(_, record)| record.is_open())
            .collect();
        assert!(
            open.len() <= 1,
            "`{}` holds {} open assignments",
            participant.agent_id,
            open.len()
        );
        if let Some((index, _)) = open.first() {
            assert_eq!(*index, participant.assignments.len() - 1);
        }
        for pair in participant.assignments.windows(2) {
            assert!(pair[0].assigned_at < pair[1].assigned_at);
        }
        for record in &participant.assignments {
            if let Some(completed) = record.completed_at {
                assert!(completed > record.assigned_at);
            }
        }
    }
    assert!(
        state.episode().settled() >= settled_before,
        "settled fell from {settled_before} to {}",
        state.episode().settled()
    );
    // A queued handoff waits only on a seat that is still working; completion
    // drains one, so a settled seat never holds a queue.
    for id in state.ledger().queues.keys() {
        assert!(
            pending.contains(&id.as_str()),
            "`{id}` is settled and holds a queue"
        );
    }
    // Woken and stalled partition the pending seats.
    let woken: Vec<String> = driver
        .pending_round(state)
        .expect("round")
        .agents()
        .iter()
        .map(|pending| pending.hive_agent_id.to_owned())
        .collect();
    for id in state.stalled() {
        assert!(!woken.contains(&id), "`{id}` is both woken and stalled");
    }
    // Quiescent implies complete, never the reverse alone.
    if state.quiescent() {
        assert!(pending.is_empty());
        assert!(state.ledger().is_drained());
    }
}

fn step(
    driver: &CompletionDriver<'_, Seat>,
    state: &DriverState,
    author: &str,
    at: Sequence,
    utterance: Utterance,
    router: &CyclingRouter,
) -> Option<DriverState> {
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: Some(router),
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    fixture()
        .block_on(driver.apply_committed(
            state,
            CommittedUtterance {
                author_id: author.into(),
                sequence: at,
                utterance,
            },
            Some(routing),
        ))
        .ok()
        .map(|transition| transition.state)
}

#[test]
fn arbitrary_event_orderings_never_corrupt_the_episode() {
    for seed in 1..120_u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let count = 2 + rng.below(3);
        let hive = hive(count);
        let driver = CompletionDriver::new(&hive, 2)
            .expect("driver")
            .with_queue_depth(1 + rng.below(3))
            .expect("depth")
            .with_broadcast_budget(Some(1 + u32::try_from(rng.below(3)).unwrap_or(0)));
        let router = CyclingRouter::default();
        let mut state = driver.start(opened(count)).expect("state");
        let mut clock = 1_u64;

        for _ in 0..80 {
            let who = IDS[rng.below(count)];
            let settled_before = state.episode().settled();
            // Mostly forward, sometimes backward: a medium that reorders is
            // exactly what the stale guard exists for, and a refusal is a pass.
            clock = if rng.below(8) == 0 {
                clock.saturating_sub(rng.below(4) as u64)
            } else {
                clock + 1
            };
            let at = Sequence(clock);
            let next = match rng.below(7) {
                0 => step(
                    &driver,
                    &state,
                    who,
                    at,
                    Utterance::CompleteEpisode {
                        message: "done".into(),
                    },
                    &router,
                ),
                1 | 2 => step(
                    &driver,
                    &state,
                    who,
                    at,
                    Utterance::Broadcast {
                        message: "handoff".into(),
                    },
                    &router,
                ),
                3 => {
                    let target = IDS[rng.below(count)];
                    step(
                        &driver,
                        &state,
                        who,
                        at,
                        Utterance::Ask {
                            to: target.into(),
                            message: "?".into(),
                        },
                        &router,
                    )
                }
                4 => step(
                    &driver,
                    &state,
                    who,
                    at,
                    Utterance::Post {
                        message: "note".into(),
                    },
                    &router,
                ),
                5 => {
                    let mut reported = state.clone();
                    reported.delivered(who, at);
                    Some(reported)
                }
                _ => {
                    let mut reported = state.clone();
                    reported.turn_started(who);
                    Some(reported)
                }
            };
            if let Some(next) = next {
                state = next;
            }
            hold(&driver, &state, settled_before);
        }
    }
}

#[test]
fn a_snapshot_round_trip_preserves_every_decision() {
    for seed in 1..40_u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1);
        let count = 2 + rng.below(3);
        let hive = hive(count);
        let driver = CompletionDriver::new(&hive, 2).expect("driver");
        let router = CyclingRouter::default();
        let mut state = driver.start(opened(count)).expect("state");

        for clock in 2..40_u64 {
            let who = IDS[rng.below(count)];
            let utterance = match rng.below(3) {
                0 => Utterance::CompleteEpisode {
                    message: "done".into(),
                },
                1 => Utterance::Broadcast {
                    message: "handoff".into(),
                },
                _ => Utterance::Ask {
                    to: IDS[rng.below(count)].into(),
                    message: "?".into(),
                },
            };
            if let Some(next) = step(&driver, &state, who, Sequence(clock), utterance, &router) {
                state = next;
            }
        }

        // A queued handoff and an open ask live only in the ledger -- the
        // episode has no record of them -- so a round trip losing anything
        // loses work.
        let json = serde_json::to_string(&state).expect("serializes");
        let restored: DriverState = serde_json::from_str(&json).expect("round-trips");
        let restored = driver.resume(restored).expect("validates");
        assert_eq!(restored.episode(), state.episode());
        assert_eq!(restored.ledger(), state.ledger());
        assert_eq!(
            restored.seen().delivered_through,
            state.seen().delivered_through
        );
        assert_eq!(
            serde_json::to_string(&restored).expect("re-serializes"),
            json
        );
    }
}
