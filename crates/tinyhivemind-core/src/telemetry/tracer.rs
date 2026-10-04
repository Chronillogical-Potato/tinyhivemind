//! The tracer: stamps events and derives them from the folds' return values.

use std::sync::atomic::{AtomicU64, Ordering};

use super::types::{Clock, RoundSeat, Stamped, TraceEvent, TraceSink};
use crate::driver::Event as ConductEvent;
use crate::hive::{HiveStep, Phase, Visibility};

/// Stamps events for one run and sends them to a sink.
///
/// Sequence numbers come from an atomic counter, so seats running concurrently
/// can share one tracer and still produce a gapless order.
pub struct Tracer<'a> {
    run: String,
    sink: &'a dyn TraceSink,
    clock: &'a dyn Clock,
    next: AtomicU64,
}

impl std::fmt::Debug for Tracer<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Tracer")
            .field("run", &self.run)
            .field("next", &self.next.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl<'a> Tracer<'a> {
    /// A tracer for the run named `run`.
    pub fn new(run: impl Into<String>, sink: &'a dyn TraceSink, clock: &'a dyn Clock) -> Self {
        Self {
            run: run.into(),
            sink,
            clock,
            next: AtomicU64::new(0),
        }
    }

    /// Stamp and record one event.
    pub fn emit(&self, event: TraceEvent) {
        let seq = self.next.fetch_add(1, Ordering::Relaxed);
        self.sink.record(&Stamped {
            run: self.run.clone(),
            seq,
            at_ms: self.clock.now_ms(),
            event,
        });
    }

    /// Record the outcome of one [`step`](crate::hive::step).
    ///
    /// A speaking round is recorded with the phase and visibility of its first
    /// turn, which every turn in a round shares.
    pub fn step(&self, step: &HiveStep) {
        self.emit(match step {
            HiveStep::Speak { turns, .. } => TraceEvent::Round {
                phase: turns.first().map_or(Phase::Deliberate, |turn| turn.phase),
                visibility: turns
                    .first()
                    .map_or(Visibility::Blind, |turn| turn.visibility),
                seats: turns
                    .iter()
                    .map(|turn| RoundSeat {
                        agent_id: turn.agent_id.clone(),
                        reason: turn.reason,
                    })
                    .collect(),
            },
            HiveStep::Converged { topic, .. } => TraceEvent::Converged {
                topic: topic.clone(),
            },
            HiveStep::Deadlocked { topics } => TraceEvent::Deadlocked {
                topics: topics.clone(),
            },
            HiveStep::Exhausted {
                spent,
                standings,
                visibility,
            } => TraceEvent::Exhausted {
                spent: *spent,
                visibility: *visibility,
                advocated: u32::try_from(standings.len()).unwrap_or(u32::MAX),
            },
            HiveStep::Idle => TraceEvent::Idle,
        });
    }

    /// Record one conductor event.
    pub fn conducted(&self, event: &ConductEvent) {
        self.emit(TraceEvent::Conducted {
            conducted: event.clone(),
        });
    }
}
