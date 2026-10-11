//! The NDJSON wire types `--run --frames` and `--compare` print.
//!
//! These are the viewer's contract (see this directory's README). Fields may be
//! added; none may be renamed or removed. `test.rs` pins each one's JSON.

use serde::Serialize;
use tinyhivemind_core::hive::{SessionAuthor, TraceKind, trace::read};

use crate::episode::{RoundRecord, TurnRecord, speech};

/// The problem a math run poses, with its exact answer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ProblemFrame {
    pub(crate) id: String,
    pub(crate) statement: String,
    pub(crate) answer: String,
}

/// One NDJSON line of a run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Frame {
    /// First line: what is running.
    Start {
        package: String,
        seed: u64,
        seats: usize,
        problem: Option<ProblemFrame>,
    },
    /// One line per speaking round.
    Round {
        round: u32,
        phase: String,
        visibility: String,
        turns: Vec<TurnFrame>,
        standings: Vec<StandingFrame>,
    },
    /// Last line: how core ended the episode.
    End {
        ending: String,
        topic: Option<String>,
        correct: Option<bool>,
        rounds: u32,
        turns: u32,
    },
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
        phase: snake(&format!("{:?}", record.phase)),
        visibility: snake(&format!("{:?}", record.visibility)),
        turns: record.turns.iter().map(|turn| turn_frame(turn, truth)).collect(),
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

fn turn_frame(turn: &TurnRecord, truth: Option<&str>) -> TurnFrame {
    let answer = advocated(&turn.seat, &turn.content);
    TurnFrame {
        seat: turn.seat.clone(),
        desk: turn.desk.clone(),
        reason: turn.reason.clone(),
        saw: turn.saw,
        of: turn.of,
        content: turn.content.clone(),
        correct: truth.zip(answer.as_deref()).map(|(truth, answer)| truth == answer),
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
            matches!(trace.kind, TraceKind::Propose | TraceKind::Support | TraceKind::Commit)
                && matches!(trace.author, SessionAuthor::Agent { .. })
        })
        .and_then(|trace| trace.topic.map(|topic| topic.0))
}

/// Lowercase a single-word `Debug` name.
fn snake(name: &str) -> String {
    name.to_lowercase()
}
