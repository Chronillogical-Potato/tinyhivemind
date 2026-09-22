//! A room that needs no model: stochastic seats, and a router that produces
//! well-formed evaluations so real acceptance still runs.

// A benchmark, not a library; see `main.rs`.
#![allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    unreachable_pub
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use tinyhivemind::responder::{PROBABILITY_SCALE, Probability};
use tinyhivemind::speech::Utterance;
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, Router, RouterFuture,
    RoutingEvaluation, RoutingRequest,
};

/// Deterministic, dependency-free, and seeded per episode so any row in a table
/// can be reproduced from its arm and index alone.
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub const fn seeded(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Parts per million, for comparing against a probability.
    pub fn parts(&mut self) -> u32 {
        u32::try_from(self.next() % u64::from(PROBABILITY_SCALE)).unwrap_or(0)
    }

    pub fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound.max(1) as u64).unwrap_or(0)
    }
}

/// What one simulated participant does with a turn.
#[derive(Clone, Copy, Debug)]
pub struct AgentModel {
    /// Turns of work its opening assignment takes.
    pub work: u32,
    /// Per-turn chance it finds work belonging to a specialist.
    pub discovery: u32,
    /// Turns each handed-off assignment takes.
    pub handoff_work: u32,
    /// Longest a turn may take, in ticks.
    ///
    /// Without this every turn lands at once and a routing call always lands
    /// after its peers have finished, so a handoff never meets a busy
    /// recipient -- the benchmark would report that queueing is never needed,
    /// which is an artifact of zero-latency turns rather than a finding.
    pub latency: u64,
    /// Longest a routing call adds, in ticks, before a broadcast lands.
    pub route_latency: u64,
}

struct Seat {
    /// Turns remaining on the assignment it currently holds.
    remaining: u32,
}

/// One simulated episode's world.
pub struct Room {
    model: AgentModel,
    seats: Vec<(String, Seat)>,
    rng: Rng,
    /// Hard wall. A chain that will not end is the thing being measured, not a
    /// reason for the benchmark to hang: past this every turn completes.
    turn_limit: u64,
    turns: u64,
}

impl Room {
    pub fn new(ids: &[String], model: AgentModel, seed: u64, turn_limit: u64) -> Self {
        Self {
            model,
            seats: ids
                .iter()
                .map(|id| {
                    (
                        id.clone(),
                        Seat {
                            remaining: model.work,
                        },
                    )
                })
                .collect(),
            rng: Rng::seeded(seed),
            turn_limit,
            turns: 0,
        }
    }

    /// Whether the room ran past its wall, which is the non-termination proxy.
    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.turns >= self.turn_limit
    }

    /// A seat was handed queued work: its next assignment is fresh.
    pub fn handed(&mut self, id: &str) {
        if let Some((_, seat)) = self.seats.iter_mut().find(|(seat, _)| seat == id) {
            seat.remaining = self.model.handoff_work;
        }
    }

    /// Run one turn: when it finishes, and what it said, each with the delay
    /// before that row lands.
    pub fn turn(&mut self, id: &str) -> (u64, Vec<(u64, Utterance)>) {
        self.turns += 1;
        let past_the_wall = self.turns >= self.turn_limit;
        let finished_at = self.rng.below(self.model.latency as usize + 1) as u64;
        let Some((_, seat)) = self.seats.iter_mut().find(|(seat, _)| seat == id) else {
            return (finished_at, Vec::new());
        };
        seat.remaining = seat.remaining.saturating_sub(1);
        let finished = seat.remaining == 0;

        if past_the_wall {
            // Every chain ends at the wall, so a table row is a cost rather
            // than a hang.
            return (
                finished_at,
                vec![(
                    0,
                    Utterance::CompleteEpisode {
                        message: "out of budget".into(),
                    },
                )],
            );
        }

        let mut events = Vec::new();
        if self.rng.parts() < self.model.discovery {
            let route_delay = self.rng.below(self.model.route_latency as usize + 1) as u64;
            events.push((
                route_delay,
                Utterance::Broadcast {
                    message: "found adjacent work".into(),
                },
            ));
        }
        if finished {
            // A recipient's next assignment is fresh work, not a repeat of
            // what it just finished.
            seat.remaining = self.model.handoff_work;
            events.push((
                0,
                Utterance::CompleteEpisode {
                    message: "assignment done".into(),
                },
            ));
        }
        (finished_at, events)
    }
}

/// A router that concentrates on one eligible candidate and spreads the rest.
///
/// It returns a *well-formed* evaluation rather than a plan, so every request
/// goes through the real `valid_domain`, the confidence gates and
/// `compose_plan`. An arm measures the acceptance policy, not a stub of it.
pub struct Spread {
    rng: Mutex<Rng>,
    /// How much mass the winner takes, in parts per million.
    concentration: u32,
    calls: AtomicU64,
}

impl Spread {
    #[must_use]
    pub fn new(seed: u64, concentration: u32) -> Self {
        Self {
            rng: Mutex::new(Rng::seeded(seed)),
            concentration,
            calls: AtomicU64::new(0),
        }
    }

    /// Routing calls made: the provider bill, one line.
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Router for Spread {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let eligible: Vec<_> = request
                .candidates
                .iter()
                .filter(|candidate| candidate.available)
                .map(|candidate| candidate.id.clone())
                .collect();
            if eligible.is_empty() {
                return Err("no eligible candidate".into());
            }
            let mut rng = self.rng.lock().expect("not poisoned");
            let winner = rng.below(eligible.len());

            // Exact fixed point: the remainder goes to `none`, so the domain
            // sums to the scale however the division falls. Shaped after a
            // live probe: real Jev on a genuine three-way choice returned
            // 0.57 / 0.42 / 0.00 / 0.01. A distribution that puts everything on
            // one candidate makes the `>20%` rule unreachable and prices width
            // at zero by construction.
            let runner = if eligible.len() > 1 {
                (winner + 1) % eligible.len()
            } else {
                winner
            };
            let mut probabilities = Vec::with_capacity(eligible.len() + 1);
            let mut spent = 0_u32;
            let remainder = PROBABILITY_SCALE - self.concentration;
            let runner_up = remainder / 10 * 8;
            let rest = u32::try_from(eligible.len().saturating_sub(2)).unwrap_or(0);
            let share = if rest == 0 {
                0
            } else {
                (remainder - runner_up) / (rest + 1)
            };
            for (index, id) in eligible.iter().enumerate() {
                let parts = if index == winner {
                    self.concentration
                } else if index == runner {
                    runner_up
                } else {
                    share
                };
                spent += parts;
                probabilities.push(CandidateProbability {
                    candidate_id: id.clone(),
                    probability: probability(parts),
                });
            }
            probabilities.push(CandidateProbability {
                candidate_id: "none".into(),
                probability: probability(PROBABILITY_SCALE - spent),
            });

            Ok(RoutingEvaluation {
                primary_responder: eligible[winner].clone(),
                primary_probabilities: probabilities,
                confidence: probability(self.concentration),
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
                model_identity: "spread".into(),
                question_schema_version: 1,
                roster_version: request.roster_version,
                disposition: EvaluationDisposition::Accepted,
            })
        })
    }
}

fn probability(parts: u32) -> Probability {
    Probability::new(parts.min(PROBABILITY_SCALE)).expect("clamped to scale")
}
