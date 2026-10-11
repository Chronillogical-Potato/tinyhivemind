//! The `hive_view` frame contract, written to `frames.ndjson` so the lab
//! viewer can replay or follow a live run.
//!
//! The field names and shapes are exactly those of
//! `crates/tinyhivemind-lang/examples/hive_view/frames.rs` (see that
//! directory's README, "Wire format"). This host only adds fields:
//! `turns[].tool_calls`, `turns[].memory`, `turns[].usage`, and the
//! `start.limits` it ran under.

use serde::Serialize;

/// The problem a run was handed. The statement is the run's own and the
/// answer is never known to the host while the hive runs.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ProblemFrame {
    pub(super) id: String,
    pub(super) statement: String,
    pub(super) answer: Option<String>,
}

/// One frame.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Frame {
    Start {
        package: String,
        seed: u64,
        seats: usize,
        problem: Option<ProblemFrame>,
        #[serde(skip_serializing_if = "Option::is_none")]
        live: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        limits: serde_json::Value,
    },
    Round {
        round: u32,
        desk: String,
        desk_round: u32,
        phase: String,
        visibility: String,
        turns: Vec<TurnFrame>,
        standings: Vec<StandingFrame>,
    },
    Referral(ReferralFrame),
    DeskEnd {
        desk: String,
        ending: String,
        topic: Option<String>,
        correct: Option<bool>,
        rounds: u32,
        turns: u32,
    },
    Usage(UsageFrame),
    End {
        ending: String,
        topic: Option<String>,
        correct: Option<bool>,
        rounds: u32,
        turns: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        desk: Option<String>,
    },
}

/// One referral, routed or refused.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ReferralFrame {
    pub(super) round: u32,
    pub(super) kind: String,
    pub(super) from_desk: String,
    pub(super) to_desk: String,
    pub(super) seat: String,
    pub(super) target: Option<String>,
    pub(super) content: String,
    pub(super) hops: u32,
    pub(super) accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reason: Option<String>,
    pub(super) sequence: Option<u64>,
}

/// The run's spend.
#[derive(Clone, Debug, Serialize)]
pub(super) struct UsageFrame {
    pub(super) calls: u64,
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) total_tokens: u64,
    pub(super) cost_usd: f64,
    pub(super) wall_ms: u64,
    pub(super) exhausted_calls: bool,
}

/// One seat's turn in a round.
#[derive(Clone, Debug, Serialize)]
pub(super) struct TurnFrame {
    pub(super) seat: String,
    pub(super) desk: String,
    pub(super) reason: String,
    pub(super) saw: usize,
    pub(super) of: usize,
    pub(super) content: String,
    pub(super) answer: Option<String>,
    pub(super) correct: Option<bool>,
    pub(super) sequence: u64,
    /// Addition: the native tools the seat called, in order.
    pub(super) tool_calls: Vec<ToolCallFrame>,
    /// Addition: what the seat recalled and remembered this turn.
    pub(super) memory: MemoryFrame,
    /// Addition: the turn's model spend.
    pub(super) usage: Option<TurnUsageFrame>,
}

/// One native tool call.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ToolCallFrame {
    pub(super) tool: String,
    pub(super) summary: String,
    pub(super) ok: bool,
}

/// One turn's memory traffic.
#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct MemoryFrame {
    pub(super) recalled: usize,
    pub(super) remembered: usize,
}

/// One turn's model spend.
#[derive(Clone, Debug, Serialize)]
pub(super) struct TurnUsageFrame {
    pub(super) model_calls: u64,
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) cost_usd: Option<f64>,
}

/// One candidate's standing on a desk.
#[derive(Clone, Debug, Serialize)]
pub(super) struct StandingFrame {
    pub(super) topic: String,
    pub(super) support: usize,
    pub(super) object: usize,
    pub(super) committed: bool,
    pub(super) supporters: Vec<String>,
}
