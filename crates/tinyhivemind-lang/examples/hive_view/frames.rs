//! The NDJSON wire types `--run --frames` and `--compare` print.
//!
//! These are the viewer's contract (see this directory's README). Fields may be
//! added; none may be renamed or removed. `test.rs` pins each one's JSON.

use serde::Serialize;
use tinyhivemind_core::hive::{SessionAuthor, TraceKind, trace::read};

use crate::desks::{DeskEnd, ReferralRecord};
use crate::episode::{RoundRecord, TurnRecord, speech};

/// The problem a math run poses, with its exact answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ProblemFrame {
    pub(crate) id: String,
    pub(crate) statement: String,
    pub(crate) answer: String,
}

/// One NDJSON line of a run.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Frame {
    /// First line: what is running.
    Start {
        package: String,
        seed: u64,
        seats: usize,
        problem: Option<ProblemFrame>,
        /// Addition: `true` on a `--live` run, absent otherwise.
        #[serde(skip_serializing_if = "Option::is_none")]
        live: Option<bool>,
        /// Addition: the model a `--live` run calls, absent otherwise.
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    /// One line per speaking round (or per referral answer, phase `answer`).
    Round {
        /// Counted across every desk of the run.
        round: u32,
        /// Addition: the desk the round ran on.
        desk: String,
        /// Addition: the round's number on that desk.
        desk_round: u32,
        phase: String,
        visibility: String,
        turns: Vec<TurnFrame>,
        standings: Vec<StandingFrame>,
    },
    /// Addition: a referral between desks, routed or refused.
    Referral(ReferralFrame),
    /// Addition: one desk's ending, before the final `end` (several desks only).
    DeskEnd {
        desk: String,
        ending: String,
        topic: Option<String>,
        correct: Option<bool>,
        rounds: u32,
        turns: u32,
    },
    /// Addition: what a `--live` run spent, just before `end`.
    Usage(UsageFrame),
    /// Last line: how core ended the episode.
    End {
        ending: String,
        topic: Option<String>,
        correct: Option<bool>,
        rounds: u32,
        turns: u32,
        /// Addition: the desk whose ending this is, on a run of several desks.
        #[serde(skip_serializing_if = "Option::is_none")]
        desk: Option<String>,
    },
}

/// A referral between desks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ReferralFrame {
    pub(crate) round: u32,
    /// `report`, `forward` or `return`.
    pub(crate) kind: String,
    pub(crate) from_desk: String,
    pub(crate) to_desk: String,
    pub(crate) seat: String,
    /// The seat core chose to receive it, when it chose one.
    pub(crate) target: Option<String>,
    pub(crate) content: String,
    pub(crate) hops: u32,
    pub(crate) accepted: bool,
    /// Present only when `accepted` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
    /// Where it landed in the receiving desk's journal; `null` if refused.
    pub(crate) sequence: Option<u64>,
}

/// What a `--live` run spent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct UsageFrame {
    pub(crate) calls: u64,
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
    pub(crate) total_tokens: u64,
    /// As `OpenRouter` reported it per call, summed; `0.0` if it reported none.
    pub(crate) cost_usd: f64,
    pub(crate) wall_ms: u64,
    /// Whether `--max-calls` stopped the run.
    pub(crate) exhausted_calls: bool,
}

/// One turn inside a round frame.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct TurnFrame {
    pub(crate) seat: String,
    pub(crate) desk: String,
    pub(crate) reason: String,
    pub(crate) saw: usize,
    pub(crate) of: usize,
    pub(crate) content: String,
    /// The topic the line proposes, supports or commits; `None` otherwise.
    pub(crate) answer: Option<String>,
    /// `answer` against ground truth; `None` when not math or no answer.
    pub(crate) correct: Option<bool>,
    /// The sequence the line was appended at (an addition to the contract).
    pub(crate) sequence: u64,
}

/// One topic's standing after a round.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct StandingFrame {
    pub(crate) topic: String,
    /// Distinct supporters left after cross-inhibition.
    pub(crate) support: usize,
    /// Advocates silenced by a grounded objection.
    pub(crate) object: usize,
    /// Whether a `!commit` names the topic.
    pub(crate) committed: bool,
    /// The supporters themselves (an addition to the contract).
    pub(crate) supporters: Vec<String>,
}

/// One package's result for one seed of a comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SeedResult {
    pub(crate) package: String,
    pub(crate) seats: usize,
    pub(crate) ending: String,
    pub(crate) correct: bool,
    pub(crate) rounds: u32,
    pub(crate) turns: u32,
    /// The decided topic, when there is one (an addition to the contract).
    pub(crate) topic: Option<String>,
}

/// One package's summary over every seed of a comparison.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SummaryRow {
    pub(crate) package: String,
    pub(crate) seats: usize,
    pub(crate) accuracy: f64,
    pub(crate) converged: f64,
    /// Additions to the contract: the other three endings' rates.
    pub(crate) deadlocked: f64,
    pub(crate) exhausted: f64,
    pub(crate) idle: f64,
    pub(crate) mean_rounds: f64,
    pub(crate) mean_turns: f64,
}

/// One NDJSON line of `--compare`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum CompareFrame {
    /// Every package on one seed.
    Seed {
        seed: u64,
        problem: String,
        results: Vec<SeedResult>,
    },
    /// Last line: the table.
    Summary { seeds: u64, rows: Vec<SummaryRow> },
}

/// The round frame for a round record; `truth` is the math answer, if any.
pub(crate) fn round(record: &RoundRecord, truth: Option<&str>) -> Frame {
    Frame::Round {
        round: record.round,
        desk: record.desk.clone(),
        desk_round: record.desk_round,
        phase: record
            .phase
            .map_or_else(|| "answer".to_owned(), |phase| snake(&format!("{phase:?}"))),
        visibility: snake(&format!("{:?}", record.visibility)),
        turns: record
            .turns
            .iter()
            .map(|turn| turn_frame(turn, truth))
            .collect(),
        standings: record
            .standings
            .iter()
            .map(|standing| StandingFrame {
                topic: standing.topic.0.clone(),
                support: standing.supporters.len(),
                object: standing.silenced.len(),
                committed: record.committed.contains(&standing.topic.0),
                supporters: standing.supporters.clone(),
            })
            .collect(),
    }
}

/// The referral frame for a routed or refused referral.
pub(crate) fn referral(record: &ReferralRecord) -> Frame {
    Frame::Referral(ReferralFrame {
        round: record.round,
        kind: record.kind.to_owned(),
        from_desk: record.from_desk.clone(),
        to_desk: record.to_desk.clone(),
        seat: record.seat.clone(),
        target: record.target.clone(),
        content: record.content.clone(),
        hops: record.hops,
        accepted: record.accepted,
        reason: record.reason.clone(),
        sequence: record.sequence,
    })
}

/// The `desk_end` frame for one desk's ending; `truth` as for [`round`].
pub(crate) fn desk_end(end: &DeskEnd, truth: Option<&str>) -> Frame {
    let topic = end.ending.topic().map(str::to_owned);
    Frame::DeskEnd {
        desk: end.desk.clone(),
        ending: end.ending.name().into(),
        correct: truth.map(|truth| topic.as_deref() == Some(truth)),
        topic,
        rounds: end.rounds,
        turns: end.turns,
    }
}

fn turn_frame(turn: &TurnRecord, truth: Option<&str>) -> TurnFrame {
    let answer = advocated(&turn.seat, &turn.content);
    TurnFrame {
        seat: turn.seat.clone(),
        desk: turn.desk.clone(),
        reason: turn.reason.clone(),
        saw: turn.saw,
        of: turn.of,
        content: turn.content.clone(),
        correct: truth
            .zip(answer.as_deref())
            .map(|(truth, answer)| truth == answer),
        answer,
        sequence: turn.sequence,
    }
}

/// The first topic a line proposes, supports or commits, read with core's
/// trace grammar.
pub(crate) fn advocated(seat: &str, content: &str) -> Option<String> {
    let message = speech(1, seat, content);
    read(std::slice::from_ref(&message))
        .into_iter()
        .find(|trace| {
            matches!(
                trace.kind,
                TraceKind::Propose | TraceKind::Support | TraceKind::Commit
            ) && matches!(trace.author, SessionAuthor::Agent { .. })
        })
        .and_then(|trace| trace.topic.map(|topic| topic.0))
}

/// Lowercase a single-word `Debug` name.
fn snake(name: &str) -> String {
    name.to_lowercase()
}
