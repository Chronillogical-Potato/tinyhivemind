//! One whole run: pick the arm, drive it, summarise it.
//!
//! [`run`] is the only place the two arms meet. Both receive the same
//! [`Env`]: one model client, one sandbox, one meter, one tracer. The
//! [`Summary`] is what `result.json` holds.

use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};

use super::board::Board;
use super::config::{Config, Mode};
use super::hive::{self, Params};
use super::llm::Llm;
use super::memory::SeatMemory;
use super::meter::Abort;
use super::roles::Role;
use super::sandbox::Exec;
use super::seat::{Env, finish_memory};
use super::session::{SessionMode, Sessions};
use super::single;

/// The outcome of a run, as written to `result.json`.
#[derive(Clone, Debug)]
pub struct Summary {
    /// `hive` or `single`.
    pub mode: &'static str,
    /// The model.
    pub model: String,
    /// Prompt tokens, all seats.
    pub tokens_in: u64,
    /// Completion tokens, all seats.
    pub tokens_out: u64,
    /// Wall clock of the whole run.
    pub wall_ms: u64,
    /// Model calls, all seats.
    pub turns: u64,
    /// Whether the agent called `complete_episode` (lead, in hive mode).
    pub completed: bool,
    /// The cap or failure that stopped the run, if any.
    pub aborted: Option<String>,
    /// Hive rounds (zero for single).
    pub rounds: u32,
    /// Seat activations (one for single).
    pub activations: u32,
    /// Largest prompt any single model call reported, all seats.
    pub max_prompt_tokens: u64,
    /// Context policy in force: the `--single-context` value for single, the
    /// hive sessions' compaction for hive.
    pub context_policy: &'static str,
    /// `persistent` or `fresh` (the hive's `--seat-session`; single is one
    /// session either way).
    pub seat_session: &'static str,
    /// `none` or `cortex`.
    pub memory: &'static str,
    /// The prompt budget the policy acts above.
    pub context_budget: u64,
    /// Times the policy masked or summarized.
    pub context_events: u64,
    /// Usage per seat.
    pub seats: Value,
}

impl Summary {
    /// The `result.json` document.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "mode": self.mode,
            "model": self.model,
            "tokens_in": self.tokens_in,
            "tokens_out": self.tokens_out,
            "wall_ms": self.wall_ms,
            "turns": self.turns,
            "completed": self.completed,
            "aborted": self.aborted,
            "rounds": self.rounds,
            "activations": self.activations,
            "max_prompt_tokens": self.max_prompt_tokens,
            "context_policy": self.context_policy,
            "context_budget": self.context_budget,
            "context_events": self.context_events,
            "seat_session": self.seat_session,
            "memory": self.memory,
            "seats": self.seats,
        })
    }
}

/// Run `config.task` on the chosen arm, with `memory` when the run has one.
pub fn run(
    config: &Config,
    llm: &Llm,
    exec: &dyn Exec,
    tracer: &Tracer<'_>,
    memory: Option<&dyn SeatMemory>,
) -> Summary {
    let started = Instant::now();
    let seats: Vec<&str> = match config.mode {
        Mode::Hive => Role::ALL.iter().map(|role| role.id()).collect(),
        Mode::Single => vec![single::SEAT],
    };
    let board = Board::new(&seats, 8);
    let turns = AtomicU64::new(0);
    let session_mode = match config.mode {
        Mode::Hive => config.seat_session,
        Mode::Single => SessionMode::Persistent,
    };
    let sessions = Sessions::new(session_mode);
    let env = Env {
        llm,
        exec,
        tracer,
        board: &board,
        turns: &turns,
        cmd_timeout: Duration::from_secs(config.cmd_timeout),
        output_limit: config.output_limit,
        sessions: &sessions,
        memory,
    };
    tracer.emit(TraceEvent::Mark {
        label: "run start".into(),
        detail: format!(
            "{} {} max_turns {} round_width {} seat_session {} memory {}",
            config.mode.name(),
            llm.model(),
            config.max_turns,
            config.round_width,
            session_mode.name(),
            if memory.is_some() { "on" } else { "off" }
        ),
    });
    let (completed, abort, rounds, activations): (bool, Option<Abort>, u32, u32) = match config.mode
    {
        Mode::Hive => {
            let report = hive::run(
                &env,
                &config.task,
                &Params {
                    round_width: config.round_width,
                    steps: config.steps_per_turn,
                    context: config.hive_settings(),
                },
            );
            (
                report.completed,
                report.abort,
                report.rounds,
                report.activations,
            )
        }
        Mode::Single => {
            let steps = usize::try_from(config.max_turns).unwrap_or(usize::MAX);
            let outcome = single::run(&env, &config.task, steps, config.single_settings());
            (outcome.completed, outcome.abort, 0, 1)
        }
    };
    if let Some(Abort::ContextOverflow(why)) = &abort {
        tracer.emit(TraceEvent::Mark {
            label: "context_overflow".into(),
            detail: why.clone(),
        });
    }
    finish_memory(&env);
    let snapshot = llm.meter().snapshot();
    let wall_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracer.emit(TraceEvent::Mark {
        label: "run end".into(),
        detail: format!("completed {completed}"),
    });
    let per_seat: serde_json::Map<String, Value> = snapshot
        .seats
        .iter()
        .map(|(seat, usage)| {
            (
                seat.clone(),
                json!({ "in": usage.input, "out": usage.output, "calls": usage.calls }),
            )
        })
        .collect();
    Summary {
        mode: config.mode.name(),
        model: llm.model().to_owned(),
        tokens_in: snapshot.input,
        tokens_out: snapshot.output,
        wall_ms,
        turns: snapshot.calls,
        completed,
        aborted: abort.map(|why| why.to_string()),
        rounds,
        activations,
        max_prompt_tokens: snapshot.max_prompt,
        context_policy: match config.mode {
            Mode::Hive => config.hive_settings().policy.name(),
            Mode::Single => config.single_context.name(),
        },
        seat_session: session_mode.name(),
        memory: if memory.is_some() {
            config.memory.name()
        } else {
            "none"
        },
        context_budget: config.context_budget,
        context_events: snapshot.context_events,
        seats: Value::Object(per_seat),
    }
}

#[cfg(test)]
mod test;
