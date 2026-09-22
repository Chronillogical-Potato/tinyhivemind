//! What the completion driver's policy costs, measured rather than asserted.
//!
//! Every arm runs the same rooms through the same `CompletionDriver` under a
//! different policy, so a difference is a difference in one thing. Nothing
//! here needs a model, a provider or a credential: the room is stochastic and
//! the router returns well-formed evaluations, so real acceptance still runs.
//!
//! ```sh
//! cargo run --release -p tinyhivemind-openhuman --example bench
//! cargo run --release -p tinyhivemind-openhuman --example bench -- --episodes 2000 --members 8
//! ```
//!
//! # Why there is no `gate` arm
//!
//! The prototype this replaces priced "busy means unreachable" against
//! "busy means queued" and found gating discards a tenth of its routing
//! spend. The driver is the queue path; gating is not a policy it offers, so
//! it is not an arm here. `docs/experiments/2026-09-21-what-the-host-loop-costs.md`
//! keeps that measurement.
//!
//! # How concurrency is modelled
//!
//! A turn takes up to `latency` ticks and a routing call up to `route_latency`
//! more, and every row a wave produces is committed in order of when it would
//! have landed. A broadcast that lands before its recipient's completion meets
//! a working seat and is queued; one that lands after meets a settled seat and
//! is assigned. That is the whole situation the queue exists for, and ordering
//! by landing time reproduces it deterministically without a task per turn.

// A benchmark, not a library: a lost decimal here ends a measurement rather
// than a host's episode, so the lints that guard production paths are relaxed
// in this example the way they are in the crate's own tests.
#![allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    unreachable_pub
)]

mod sim;

use std::fmt::Write as _;
use std::sync::OnceLock;

use openhuman_embed::{Agent, AgentSpec, Runtime, Workspace};
use tinyhivemind::responder::Probability;
use tinyhivemind::speech::Utterance;
use tinyhivemind::{Conversation, Sequence, desk::Desk};
use tinyhivemind_embed::{RouteCandidate, RoutingPolicy};
use tinyhivemind_hive::CompletionEpisodeState;
use tinyhivemind_openhuman::{
    AgentBinding, BroadcastRouting, CommittedUtterance, CompletionDriver, DriverState, Error,
    HiveGraph, HostAction, OpenHumanHive,
};

use sim::{AgentModel, Room, Spread};

/// One policy under test, and what it is meant to isolate.
struct Arm {
    name: &'static str,
    cap: Option<u32>,
    width: usize,
}

const ARMS: [Arm; 4] = [
    // The default: a busy recipient is queued for, budget of three per assignment.
    Arm {
        name: "queue",
        cap: Some(3),
        width: 2,
    },
    // The bound that makes the termination measure well-founded, removed.
    Arm {
        name: "queue-nocap",
        cap: None,
        width: 2,
    },
    // Width is a multiplier on this path, so it is priced separately.
    Arm {
        name: "queue-w1",
        cap: Some(3),
        width: 1,
    },
    Arm {
        name: "queue-w4",
        cap: Some(3),
        width: 4,
    },
];

/// What one episode cost, and where it went.
#[derive(Clone, Copy, Debug, Default)]
struct Counters {
    turns: u64,
    routes: u64,
    waves: u64,
    assigned: u64,
    queued: u64,
    drained: u64,
    discharged: u64,
    unplaced: u64,
    peak_queue: usize,
}

#[derive(Default)]
struct Tally {
    episodes: u32,
    quiescent: u32,
    stalled: u32,
    errored: u32,
    exhausted: u32,
    counters: Counters,
}

enum Settled {
    Quiescent,
    Stalled(Vec<String>),
}

fn main() {
    let mut episodes = 400_u32;
    let mut members = 5_usize;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--episodes" => episodes = args.next().and_then(|v| v.parse().ok()).unwrap_or(episodes),
            "--members" => members = args.next().and_then(|v| v.parse().ok()).unwrap_or(members),
            other => {
                eprintln!("unknown flag `{other}`");
                std::process::exit(2);
            }
        }
    }

    println!(
        "{episodes} episodes, {members} members, same rooms per arm, through CompletionDriver.\n\
         quiescent%: reached a real end. stalled%: pending seats nothing will wake.\n\
         exhausted%: hit the turn wall. routes/ep is the provider bill; waves/ep is\n\
         depth against turns/ep's width. queued: handoffs that met a working seat.\n"
    );

    let mut table = String::new();
    let _ = writeln!(
        table,
        "{:<12}{:>11}{:>16}{:>9}{:>9}{:>10}{:>10}{:>9}{:>8}{:>8}{:>8}{:>7}{:>9}{:>7}",
        "arm",
        "quiescent%",
        "95% CI",
        "stalled%",
        "exhaus%",
        "turns/ep",
        "routes/ep",
        "waves/ep",
        "assign",
        "queued",
        "drained",
        "disch",
        "unplaced",
        "peakQ"
    );

    for arm in &ARMS {
        let tally = run_arm(arm, episodes, members);
        let rate = f64::from(tally.quiescent) / f64::from(tally.episodes);
        let (low, high) = wilson(tally.quiescent, tally.episodes);
        let per = f64::from(tally.episodes);
        let _ = writeln!(
            table,
            "{:<12}{:>11.1}{:>16}{:>9.1}{:>9.1}{:>10.1}{:>10.2}{:>9.1}{:>8.2}{:>8.2}{:>8.2}{:>7.2}{:>9.2}{:>7}",
            arm.name,
            rate * 100.0,
            format!("{:.1}-{:.1}", low * 100.0, high * 100.0),
            f64::from(tally.stalled) / per * 100.0,
            f64::from(tally.exhausted) / per * 100.0,
            tally.counters.turns as f64 / per,
            tally.counters.routes as f64 / per,
            tally.counters.waves as f64 / per,
            tally.counters.assigned as f64 / per,
            tally.counters.queued as f64 / per,
            tally.counters.drained as f64 / per,
            tally.counters.discharged as f64 / per,
            tally.counters.unplaced as f64 / per,
            tally.counters.peak_queue,
        );
        if tally.errored > 0 {
            let _ = writeln!(
                table,
                "{:<12}  {} episodes ended in a driver error",
                "", tally.errored
            );
        }
    }
    print!("{table}");
}

/// One `OpenHuman` runtime for the process: the driver binds real agent handles,
/// and building them is the only thing here that is not a fold.
fn agents(count: usize) -> &'static [Agent] {
    static AGENTS: OnceLock<Vec<Agent>> = OnceLock::new();
    let built = AGENTS.get_or_init(|| {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let runtime = executor
            .block_on(
                Runtime::builder()
                    .workspace(Workspace::Ephemeral)
                    .api_key("th_bench")
                    .build(),
            )
            .expect("an ephemeral OpenHuman runtime needs no credential");
        (0..16)
            .map(|index| {
                runtime
                    .agent(AgentSpec::new(format!("a{index}")))
                    .expect("agent")
            })
            .collect()
    });
    assert!(count <= built.len(), "at most {} members", built.len());
    &built[..count]
}

fn hive(ids: &[String]) -> OpenHumanHive {
    OpenHumanHive::new(
        HiveGraph::new(
            Desk {
                id: "desk".into(),
                name: "Desk".into(),
                description: None,
                members: ids.to_vec(),
                responder_mode: tinyhivemind::desk::ResponderMode::Auto,
            },
            ids.iter()
                .map(|id| RouteCandidate {
                    id: id.clone(),
                    label: id.clone(),
                    role: None,
                    description: None,
                    capabilities: Vec::new(),
                    learned_topics: Vec::new(),
                    available: true,
                })
                .collect(),
        ),
        ids.iter()
            .zip(agents(ids.len()))
            .map(|(id, agent)| AgentBinding::new(id.clone(), agent.clone()))
            .collect(),
    )
    .expect("the benchmark desk validates")
}

fn run_arm(arm: &Arm, episodes: u32, members: usize) -> Tally {
    let executor = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a current-thread runtime needs no configuration");
    let ids: Vec<String> = (0..members).map(|i| format!("a{i}")).collect();
    let hive = hive(&ids);
    let driver = CompletionDriver::new(&hive, arm.width)
        .expect("nonzero width")
        .with_queue_depth(3)
        .expect("nonzero depth")
        .with_broadcast_budget(arm.cap);
    let route_policy = routing(arm.width);
    let mut tally = Tally::default();

    for episode in 0..episodes {
        // Same seed across arms, so a difference is the policy and not the room.
        let seed = u64::from(episode).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut room = Room::new(
            &ids,
            AgentModel {
                // One turn is one assignment: the protocol has no "still
                // working" event, so an assignment must fit in a turn.
                work: 1,
                // Three in ten turns find work belonging to a specialist.
                discovery: 300_000,
                handoff_work: 1,
                // An agent turn is a whole agentic loop; routing is one model
                // call a live probe put at about a second. That ratio decides
                // whether a handoff ever meets a busy recipient.
                latency: 24,
                route_latency: 2,
            },
            seed,
            (members as u64) * 40,
        );
        let router = Spread::new(seed ^ 0x5DEE_CE66, 570_000);
        let routing = BroadcastRouting {
            primary: Some(&router),
            reasoning: None,
            policy: &route_policy,
            roster_version: 1,
            thread_context: &[],
        };
        let mut counters = Counters::default();
        let outcome = executor.block_on(run_episode(
            &driver,
            &ids,
            &mut room,
            routing,
            &mut counters,
        ));
        counters.routes = router.calls();

        tally.episodes += 1;
        match outcome {
            Ok(Settled::Quiescent) => tally.quiescent += 1,
            Ok(Settled::Stalled(seats)) => {
                tally.stalled += 1;
                if tally.stalled <= 2 {
                    eprintln!(
                        "[{}] episode {episode}: stalled on {}",
                        arm.name,
                        seats.join(", ")
                    );
                }
            }
            Err(error) => {
                tally.errored += 1;
                if tally.errored <= 2 {
                    eprintln!("[{}] episode {episode}: {error}", arm.name);
                }
            }
        }
        if room.exhausted() {
            tally.exhausted += 1;
        }
        tally.counters.turns += counters.turns;
        tally.counters.routes += counters.routes;
        tally.counters.waves += counters.waves;
        tally.counters.assigned += counters.assigned;
        tally.counters.queued += counters.queued;
        tally.counters.drained += counters.drained;
        tally.counters.discharged += counters.discharged;
        tally.counters.unplaced += counters.unplaced;
        tally.counters.peak_queue = tally.counters.peak_queue.max(counters.peak_queue);
    }
    tally
}

/// The host loop, as a host would write it: propose a round, run it, commit
/// what it said in landing order, report delivery, repeat until quiescent.
async fn run_episode(
    driver: &CompletionDriver<'_>,
    ids: &[String],
    room: &mut Room,
    routing: BroadcastRouting<'_>,
    counters: &mut Counters,
) -> Result<Settled, Error> {
    let mut state = driver.start(
        CompletionEpisodeState::opened(
            Conversation {
                desk_id: "desk".into(),
                desk_name: "Desk".into(),
                thread_root: None,
            },
            Sequence(1),
            ids,
        )
        .expect("unique nonblank participants"),
    )?;
    let mut sequence = 1_u64;

    loop {
        if state.quiescent() {
            return Ok(Settled::Quiescent);
        }
        let round = driver.pending_round(&state)?;
        let seats: Vec<String> = round
            .agents()
            .iter()
            .map(|pending| pending.hive_agent_id.to_owned())
            .collect();
        if seats.is_empty() {
            return Ok(Settled::Stalled(state.stalled()));
        }
        counters.waves += 1;

        // Every row the wave produces, tagged with when it lands.
        let mut landing: Vec<(u64, String, Utterance)> = Vec::new();
        for seat in &seats {
            // A turn is shown everything committed so far before it starts;
            // that is what a completion is checked against.
            state.delivered(seat, Sequence(sequence));
            state.turn_started(seat);
            counters.turns += 1;
            let (finished_at, events) = room.turn(seat);
            for (delay, utterance) in events {
                landing.push((finished_at + delay, seat.clone(), utterance));
            }
        }
        landing.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

        for (_, seat, utterance) in landing {
            state = commit(
                driver,
                &state,
                ids,
                room,
                routing,
                counters,
                &mut sequence,
                &seat,
                utterance,
            )
            .await?;
        }
        for seat in &seats {
            state.delivered(seat, Sequence(sequence));
        }
    }
}

/// Commit one row, count what it did, and let the host's one decision run:
/// an author whose budget is spent keeps the work and is finished with it.
#[allow(clippy::too_many_arguments)]
async fn commit(
    driver: &CompletionDriver<'_>,
    state: &DriverState,
    ids: &[String],
    room: &mut Room,
    routing: BroadcastRouting<'_>,
    counters: &mut Counters,
    sequence: &mut u64,
    seat: &str,
    utterance: Utterance,
) -> Result<DriverState, Error> {
    *sequence += 1;
    let is_broadcast = utterance.broadcasting();
    let event = CommittedUtterance {
        author_id: seat.to_owned(),
        sequence: Sequence(*sequence),
        utterance,
    };
    let before = queue_lengths(state, ids);
    let transition = match driver.apply_committed(state, event, Some(routing)).await {
        Ok(transition) => transition,
        Err(Error::BudgetSpent { .. }) => {
            counters.discharged += 1;
            *sequence += 1;
            driver
                .apply_committed(
                    state,
                    CommittedUtterance {
                        author_id: seat.to_owned(),
                        sequence: Sequence(*sequence),
                        utterance: Utterance::CompleteEpisode {
                            message: "budget spent; keeping the work".into(),
                        },
                    },
                    None,
                )
                .await?
        }
        Err(error) => return Err(error),
    };
    let after = queue_lengths(&transition.state, ids);
    let queued_now: usize = after
        .iter()
        .zip(&before)
        .map(|(a, b)| a.saturating_sub(*b))
        .sum();
    counters.queued += queued_now as u64;
    counters.peak_queue = counters
        .peak_queue
        .max(after.iter().copied().max().unwrap_or(0));
    let mut routed = false;
    for action in &transition.actions {
        match action {
            HostAction::RunAgents { agent_ids, .. } => {
                routed = true;
                let assigned = agent_ids
                    .iter()
                    .filter(|id| assigned_at(&transition.state, id) == Some(Sequence(*sequence)))
                    .count();
                counters.assigned += assigned as u64;
            }
            HostAction::DeliverHandoff { agent_id, .. } => {
                counters.drained += 1;
                room.handed(agent_id);
            }
            HostAction::DeliverDm { .. } => {}
        }
    }
    if is_broadcast && !routed {
        counters.unplaced += 1;
    }
    Ok(transition.state)
}

fn queue_lengths(state: &DriverState, ids: &[String]) -> Vec<usize> {
    ids.iter().map(|id| state.ledger().queue_len(id)).collect()
}

fn assigned_at(state: &DriverState, id: &str) -> Option<Sequence> {
    state
        .episode()
        .participants
        .iter()
        .find(|participant| participant.agent_id == id)
        .and_then(|participant| participant.open())
        .map(|record| record.assigned_at)
}

fn routing(round_width: usize) -> RoutingPolicy {
    RoutingPolicy {
        // Calibrated below what real Jev returns on a genuine three-way choice,
        // which a live probe measured at 0.42 -- a 0.6 floor escalates almost
        // everything and measures the fallback path instead of the policy.
        minimum_confidence: probability(350_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(500_000),
        high_impact_threshold: probability(700_000),
        round_width,
        choice_option_limit: 8,
    }
}

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("parts within scale")
}

/// Wilson score interval: honest at the rates an arm actually reaches, where
/// the normal approximation is not.
fn wilson(successes: u32, total: u32) -> (f64, f64) {
    if total == 0 {
        return (0.0, 0.0);
    }
    let n = f64::from(total);
    let p = f64::from(successes) / n;
    let z = 1.959_963_984_540_054_f64;
    let denominator = z.mul_add(z / n, 1.0);
    let centre = z.mul_add(z / (2.0 * n), p) / denominator;
    let spread = z * ((p * (1.0 - p) / n) + (z * z / (4.0 * n * n))).sqrt() / denominator;
    ((centre - spread).max(0.0), (centre + spread).min(1.0))
}
