//! The hive arm: rounds of bounded-width concurrent seat activations.
//!
//! Scheduling is driven by what the desk says. The lead wakes first; a seat
//! wakes when a message `@mentions` or `ask`s it or when a `broadcast` is
//! routed to it; the lead wakes again only once the queue has drained and a
//! teammate has reported, so it is not re-read on every report. A round runs at
//! most `round_width` seats at once on scoped threads, which is core's bound
//! on concurrent turns (charter rule 3). A seat's first activation opens its
//! session with [`Board::briefing_view`](super::board::Board::briefing_view)
//! (pins, digest, live tail). With persistent sessions a later activation
//! resumes that session with only
//! [`Board::delta`](super::board::Board::delta), the rows teammates committed
//! since the seat's watermark, so the seat keeps every command it ran; with
//! `--seat-session fresh` every activation starts from the briefing again.
//!
//! The run ends when the lead calls `complete_episode` (`converged`), when a
//! cap aborts it (`exhausted`), or when the desk stays idle after the lead
//! was nudged once (`idle`).

use std::sync::Arc;

use tinyhivemind_core::hive::{BidReason, Phase, TopicId, Visibility};
use tinyhivemind_core::telemetry::{RoundSeat, TraceEvent};

use super::context::Settings;
use super::meter::Abort;
use super::roles::{Role, hive_rejoin, hive_system, hive_turn};
use super::seat::{Activation, Env, Outcome, run as run_seat};
use super::session::SeatSession;
use super::tools::{HIVE_TOOLS, tool_list};

/// Knobs of the hive arm.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Most seats activated concurrently in one round.
    pub round_width: usize,
    /// Model calls one activation may make.
    pub steps: usize,
    /// The prompt budget and the compaction that enforces it on each seat's
    /// session.
    pub context: Settings,
}

/// A queued activation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Wake {
    /// The seat to run.
    pub seat: String,
    /// Why it was chosen.
    pub reason: BidReason,
    /// One line of context for the seat's opening message.
    pub note: String,
}

/// How the hive run ended.
#[derive(Debug, Default)]
pub struct Report {
    /// The lead completed the episode.
    pub completed: bool,
    /// The cap or failure that stopped it.
    pub abort: Option<Abort>,
    /// Rounds run.
    pub rounds: u32,
    /// Seat activations run.
    pub activations: u32,
}

/// Queue a wake unless that seat is already waiting.
pub fn enqueue(queue: &mut Vec<Wake>, wake: Wake) {
    if !queue.iter().any(|queued| queued.seat == wake.seat) {
        queue.push(wake);
    }
}

/// Take up to `width` wakes from the front of the queue.
pub fn take_round(queue: &mut Vec<Wake>, width: usize) -> Vec<Wake> {
    let n = width.max(1).min(queue.len());
    queue.drain(..n).collect()
}

/// Pick the seat a broadcast goes to: the best keyword match among the
/// teammates other than the author, defaulting to the implementer.
///
/// This stands in for core's semantic router, which needs a model call; see
/// the findings log.
#[must_use]
pub fn route_broadcast(author: &str, message: &str) -> &'static str {
    let lower = message.to_lowercase();
    let score = |role: Role| -> usize {
        let words: &[&str] = match role {
            Role::Tester => &["test", "verify", "reproduce", "run ", "check", "pytest"],
            Role::Reviewer => &["review", "audit", "regress", "edge case", "diff"],
            Role::Implementer => &[
                "fix",
                "implement",
                "edit",
                "change",
                "patch",
                "write",
                "add",
            ],
            Role::Lead => &[],
        };
        words.iter().filter(|word| lower.contains(*word)).count()
    };
    Role::ALL
        .into_iter()
        .filter(|role| role.id() != author && *role != Role::Lead)
        .max_by_key(|role| {
            (
                score(*role),
                *role == Role::Implementer,
                *role == Role::Tester,
            )
        })
        .map_or(Role::Implementer.id(), Role::id)
}

/// Run the hive on `task`.
pub fn run(env: &Env<'_>, task: &str, params: &Params) -> Report {
    let tools = tool_list(HIVE_TOOLS);
    let mut report = Report::default();
    let mut queue = vec![Wake {
        seat: Role::Lead.id().into(),
        reason: BidReason::Addressed,
        note: "The task is new. Plan, then delegate with broadcast.".into(),
    }];
    let mut unreported = 0_usize;
    let mut nudged = false;
    loop {
        if queue.is_empty() {
            if unreported > 0 {
                enqueue(
                    &mut queue,
                    lead_wake("Teammates reported; decide the next step."),
                );
            } else if !nudged {
                nudged = true;
                enqueue(
                    &mut queue,
                    lead_wake("The desk is idle. Delegate or complete."),
                );
            } else {
                env.tracer.emit(TraceEvent::Idle);
                break;
            }
        }
        let round = take_round(&mut queue, params.round_width);
        env.tracer.emit(TraceEvent::Round {
            phase: Phase::Deliberate,
            visibility: Visibility::Full,
            seats: round
                .iter()
                .map(|wake| RoundSeat {
                    agent_id: wake.seat.clone(),
                    reason: wake.reason,
                })
                .collect(),
        });
        report.rounds += 1;
        let results = run_round(env, task, params, &tools, &round);
        let mut spoke_non_lead = 0;
        let mut lead_ran = false;
        for (wake, outcome) in round.iter().zip(results) {
            report.activations += 1;
            if outcome.abort.is_some() {
                report.abort = outcome.abort.clone();
            }
            let is_lead = wake.seat == Role::Lead.id();
            lead_ran |= is_lead;
            let Some(done) = outcome.spoke else { continue };
            nudged = false;
            if is_lead {
                report.completed |= outcome.completed;
            } else {
                spoke_non_lead += 1;
            }
            for target in &done.addressed {
                if Role::from_id(target).is_some() {
                    enqueue(
                        &mut queue,
                        Wake {
                            seat: target.clone(),
                            reason: BidReason::Addressed,
                            note: format!("@{} addressed you.", wake.seat),
                        },
                    );
                }
            }
            if done.broadcasting && done.addressed.is_empty() {
                let target = route_broadcast(&wake.seat, &done.content);
                enqueue(
                    &mut queue,
                    Wake {
                        seat: target.into(),
                        reason: BidReason::Salience,
                        note: format!("@{} routed work to you.", wake.seat),
                    },
                );
            }
        }
        unreported = if lead_ran {
            spoke_non_lead
        } else {
            unreported + spoke_non_lead
        };
        if let Some(generation) = env.board.maintain() {
            env.tracer.emit(TraceEvent::Checkpoint {
                label: format!("digest-{generation}"),
            });
        }
        if report.completed {
            env.tracer.emit(TraceEvent::Converged {
                topic: TopicId::from("task"),
            });
            break;
        }
        if report.abort.is_some() {
            env.tracer.emit(TraceEvent::Exhausted {
                spent: u32::try_from(env.llm.meter().snapshot().calls).unwrap_or(u32::MAX),
                visibility: Visibility::Full,
                advocated: 0,
            });
            break;
        }
    }
    report
}

fn lead_wake(note: &str) -> Wake {
    Wake {
        seat: Role::Lead.id().into(),
        reason: BidReason::Quiet,
        note: note.into(),
    }
}

fn run_round(
    env: &Env<'_>,
    task: &str,
    params: &Params,
    tools: &[serde_json::Value],
    round: &[Wake],
) -> Vec<Outcome> {
    let tools = Arc::new(tools.to_vec());
    std::thread::scope(|scope| {
        let handles: Vec<_> = round
            .iter()
            .map(|wake| {
                let tools = Arc::clone(&tools);
                scope.spawn(move || {
                    // The queue dedupes by seat, so this round is the only
                    // one holding this seat's session.
                    let mut session = env.sessions.take(&wake.seat);
                    let out = activate(env, task, params, &tools, wake, &mut session);
                    env.sessions.put(&wake.seat, session);
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_default())
            .collect()
    })
}

/// Longest memory focus taken from a wake, in characters.
const FOCUS_CHARS: usize = 600;

/// One seat's activation: open or resume its session from the desk, then run.
fn activate(
    env: &Env<'_>,
    task: &str,
    params: &Params,
    tools: &[serde_json::Value],
    wake: &Wake,
    session: &mut SeatSession,
) -> Outcome {
    let role = Role::from_id(&wake.seat).unwrap_or(Role::Implementer);
    let (view, user) = if session.is_new() {
        let view = env.board.briefing_view(&wake.seat);
        let user = hive_turn(&wake.seat, &view.text, &wake.note);
        (view, user)
    } else {
        let view = env
            .board
            .delta(&wake.seat, session.read_through, session.pins_seen);
        let user = hive_rejoin(&wake.seat, &view.text, &wake.note);
        (view, user)
    };
    session.read_through = Some(view.through);
    session.pins_seen = view.pins;
    let focus: String = format!("{}\n{}", wake.note, view.text)
        .chars()
        .take(FOCUS_CHARS)
        .collect();
    run_seat(
        env,
        &Activation {
            seat: &wake.seat,
            system: hive_system(role, task, env.sessions.mode()),
            user,
            shown_rows: view.rows,
            focus,
            tools: tools.to_vec(),
            speaking: HIVE_TOOLS,
            steps: params.steps,
            implicit_post: true,
            context: params.context,
        },
        session,
    )
}

#[cfg(test)]
mod test;
