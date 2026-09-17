//! Exact `TypeSafe` System One request and response wire types used by routing.

use std::{collections::BTreeMap, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::TransportError;

/// One System One evaluation request.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SystemOneRequest {
    /// Structured application state.
    pub state: Value,
    /// Provider model identity or alias.
    pub model: String,
    /// Independently evaluated typed questions.
    pub questions: BTreeMap<String, Question>,
}

/// A routing question supported by this adapter.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// Select one mutually exclusive alternative.
    Choice {
        /// Complete decision instruction.
        instructions: Value,
        /// Alternatives and their optional rubrics.
        criteria: BTreeMap<String, Option<Value>>,
    },
    /// Judge one independent yes/no condition.
    Noul {
        /// Complete yes/no instruction.
        instructions: Value,
        /// Optional descriptions of true and false.
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
}

/// Optional true/false rubric for a Noul.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NoulCriteria {
    /// Meaning of a probability near one.
    #[serde(rename = "true")]
    pub true_description: String,
    /// Meaning of a probability near zero.
    #[serde(rename = "false")]
    pub false_description: String,
}

/// One System One evaluation response.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct SystemOneResponse {
    /// Model that performed the evaluation.
    pub model: String,
    /// Typed answers keyed by request question id.
    pub answers: BTreeMap<String, SystemOneAnswer>,
    /// Provider token accounting.
    pub usage: TokenUsage,
}

/// Token accounting returned by System One.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct TokenUsage {
    /// Input tokens charged.
    pub input_tokens: u64,
    /// Output tokens charged.
    pub output_tokens: u64,
}

/// Typed System One answer variants used by routing.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemOneAnswer {
    /// Choice answer with its complete distribution.
    Choice(ChoiceAnswer),
    /// Noul probability of yes.
    Noul(NoulAnswer),
}

/// One Choice answer.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ChoiceAnswer {
    /// Highest-probability alternative.
    pub choice: String,
    /// Complete probability distribution.
    pub probabilities: BTreeMap<String, f64>,
    /// Distribution concentration.
    pub confidence: f64,
}

/// One Noul answer.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct NoulAnswer {
    /// Probability that the answer is yes.
    pub noul: f64,
}

/// Executor-neutral future returned by [`SystemOneTransport`].
pub type SystemOneTransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SystemOneResponse, TransportError>> + Send + 'a>>;

/// The sole waiting port needed by [`crate::JevRouter`].
pub trait SystemOneTransport: Send + Sync {
    /// Evaluate one exact System One request.
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a>;
}
