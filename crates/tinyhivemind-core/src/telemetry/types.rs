//! The wire types of a run's telemetry: one stamped event per thing worth
//! profiling, and the two ports a host implements to receive them.

use serde::{Deserialize, Serialize};

use crate::driver::Event as ConductEvent;
use crate::hive::{BidReason, Phase, TopicId, Visibility};

/// One thing that happened in a run.
///
/// The variants mirror the values the folds already return — a round, a
/// conductor event — plus the three facts only a host can observe: how long a
/// turn took, what it cost in tokens, and which tool it called.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TraceEvent {
    /// A round of seats was authorized to take the floor together.
    Round {
        /// Which class of turn the round is.
        phase: Phase,
        /// How much of the transcript the round could see.
        visibility: Visibility,
        /// The seats, in desk order, with why each won the floor.
        seats: Vec<RoundSeat>,
    },
    /// One topic carried and the room recorded it.
    Converged {
        /// The topic that carried.
        topic: TopicId,
    },
    /// Two or more topics carried and nobody broke the tie.
    Deadlocked {
        /// Every tied topic.
        topics: Vec<TopicId>,
    },
    /// The turn budget ran out.
    Exhausted {
        /// Turns taken.
        spent: u32,
        /// How much of itself the room could see at the end.
        visibility: Visibility,
        /// Topics that were advocated when the budget ran out.
        advocated: u32,
    },
    /// Nobody's urge cleared their threshold.
    Idle,
    /// The conductor reported something (a nudge, park, broadcast, handoff).
    Conducted {
        /// What it reported.
        conducted: ConductEvent,
    },
    /// A seat's turn began.
    TurnStarted {
        /// The seat.
        seat: String,
    },
    /// A seat's turn ended.
    TurnFinished {
        /// The seat.
        seat: String,
        /// Prompt tokens the turn consumed.
        input_tokens: u64,
        /// Completion tokens the turn produced.
        output_tokens: u64,
        /// Wall time the host measured for the turn.
        latency_ms: u64,
    },
    /// A seat called a tool.
    ToolCall {
        /// The seat.
        seat: String,
        /// The tool name.
        tool: String,
        /// Wall time the host measured for the call.
        latency_ms: u64,
        /// Whether the call was refused.
        refused: bool,
    },
    /// A durable state was captured, for resume or replay.
    Checkpoint {
        /// A host-chosen name for what was captured.
        label: String,
    },
    /// Anything the host wants on the timeline that has no variant here.
    Mark {
        /// Short label.
        label: String,
        /// Free detail, empty when there is none.
        detail: String,
    },
}

/// One seat in a [`TraceEvent::Round`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RoundSeat {
    /// The agent that won the floor.
    pub agent_id: String,
    /// Why it won.
    pub reason: BidReason,
}

/// An event with its place in the run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Stamped {
    /// The run this belongs to, so several can share one sink.
    pub run: String,
    /// Position in the run, from zero, with no gaps.
    pub seq: u64,
    /// Milliseconds on the host's clock when it was recorded.
    pub at_ms: u64,
    /// What happened.
    #[serde(flatten)]
    pub event: TraceEvent,
}

/// Where stamped events go.
///
/// Implemented by the host: a file, a channel, a span exporter. Recording
/// must not block or fail the run, so it returns nothing and takes `&self`.
pub trait TraceSink: Send + Sync {
    /// Receive one event.
    fn record(&self, event: &Stamped);
}

/// The host's clock.
///
/// The core owns no clock, so a host supplies one. A test supplies a manual
/// one, which makes a trace deterministic.
pub trait Clock: Send + Sync {
    /// Milliseconds since any fixed origin the host chooses.
    fn now_ms(&self) -> u64;
}
