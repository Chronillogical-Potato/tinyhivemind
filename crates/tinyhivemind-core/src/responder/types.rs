//! Stable responder selection inputs and decisions.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::fmt;

use crate::mention::Mention;

/// Fixed-point probability scale used at model boundaries.
pub const PROBABILITY_SCALE: u32 = 1_000_000;

/// Probability in integer parts per million.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Probability(u32);

impl<'de> Deserialize<'de> for Probability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let parts = u32::deserialize(deserializer)?;
        Self::new(parts).ok_or_else(|| D::Error::custom("probability exceeds 1000000"))
    }
}

impl Probability {
    /// Zero probability.
    pub const ZERO: Self = Self(0);
    /// Certain probability.
    pub const ONE: Self = Self(PROBABILITY_SCALE);

    /// Construct a bounded probability.
    #[must_use]
    pub const fn new(parts_per_million: u32) -> Option<Self> {
        if parts_per_million <= PROBABILITY_SCALE {
            Some(Self(parts_per_million))
        } else {
            None
        }
    }

    /// Return integer parts per million.
    #[must_use]
    pub const fn parts(self) -> u32 {
        self.0
    }

    pub(super) const fn valid(self) -> bool {
        self.0 <= PROBABILITY_SCALE
    }
}

/// Descriptive selector input for one active desk member.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectorCandidate {
    /// Canonical agent id.
    pub id: String,
    /// Human-readable agent label.
    pub label: String,
    /// Team role supplied to the selector.
    pub role: String,
    /// Optional short capability description.
    #[serde(deserialize_with = "deserialize_required_option")]
    pub description: Option<String>,
}

/// Whether a model-assisted selection rung may run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionPolicy {
    /// The runtime may invoke its selector port.
    Allowed,
    /// Selection is disabled and the deterministic fallback wins.
    Disabled,
}

/// Caller-owned inputs to the pure responder ladder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResponderRequest {
    /// The raw authored message.
    pub message: String,
    /// Stored chat identity, or no identity for General.
    #[serde(deserialize_with = "deserialize_required_option")]
    pub chat: Option<String>,
    /// Already-resolved mentions for the message.
    pub mentions: Vec<Mention>,
    /// Canonical id of the host's orchestrator agent.
    pub orchestrator_id: String,
    /// Whether model-assisted selection is enabled for this request.
    pub selection_policy: SelectionPolicy,
    /// Minimum distribution confidence accepted from a selector.
    pub minimum_selection_confidence: Probability,
}

/// The complete, bounded input visible to a model selector.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectionRequest {
    /// The raw authored message.
    pub message: String,
    /// Canonical desk id.
    pub desk_id: String,
    /// Effective active candidates in desk order.
    pub candidates: Vec<SelectorCandidate>,
    /// Minimum confidence the host may accept from its evaluator.
    pub minimum_confidence: Probability,
}

/// One candidate's probability in a typed selector result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CandidateProbability {
    /// Canonical candidate id.
    pub candidate_id: String,
    /// Probability assigned to the candidate.
    pub probability: Probability,
}

/// Typed selector result consumed without parsing generated text.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectionEvaluation {
    /// Highest-probability candidate selected by the evaluator.
    pub choice: String,
    /// Complete probability distribution across requested candidates.
    pub probabilities: Vec<CandidateProbability>,
    /// Distribution concentration, not correctness probability.
    pub confidence: Probability,
}

/// The ladder rung that produced a responder.
///
/// The `Display` rendering is a sentence an agent may repeat to a person. It
/// discloses nothing a refusal would have to withhold: the ladder never
/// declines, so a rung always arrives beside the responder id it explains, and
/// no rung can be probed for a fact about anybody else. See [ADR 0009].
///
/// [ADR 0009]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/adr/0009-a-refusal-renders-what-the-caller-already-holds.md
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponderRung {
    /// A direct, active agent mention.
    ExplicitMention,
    /// Model-assisted selection or its deterministic fallback.
    AutoSelection,
    /// The first effective active member of a desk.
    DeskDefault,
    /// A direct-message or bare agent chat identity.
    DirectAgent,
    /// The host's orchestrator fallback.
    Orchestrator,
}

/// What happened at the optional selector boundary.
///
/// The `Display` rendering is a sentence an agent may repeat to a person.
/// [`Self::Unavailable`] and [`Self::InvalidOutput`] are worded apart on
/// purpose: both describe the host's own selector and a fallback that has
/// already produced an id, so neither reports whether some named agent exists.
/// See [ADR 0009].
///
/// [ADR 0009]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/adr/0009-a-refusal-renders-what-the-caller-already-holds.md
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionDisposition {
    /// No model selection applied to this rung.
    NotApplicable,
    /// A selector returned a valid candidate.
    Selected,
    /// Selection policy disabled the model rung.
    Disabled,
    /// No selector was available or it failed.
    Unavailable,
    /// Selector output did not name exactly one candidate.
    InvalidOutput,
}

impl fmt::Display for ResponderRung {
    /// Render why this agent is the one answering.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ExplicitMention => "the agent this message named answered it",
            Self::AutoSelection => "this desk picked whoever suited the message best",
            Self::DeskDefault => {
                "nobody in particular was named, so this desk's first agent answered"
            }
            Self::DirectAgent => "this conversation has one agent, and it answered",
            Self::Orchestrator => "no desk agent applied here, so the coordinator answered",
        })
    }
}

/// The single responder selected for one input message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResponderDecision {
    /// Canonical active agent id.
    pub responder_id: String,
    /// Ladder rung that selected the id.
    pub rung: ResponderRung,
    /// Selector outcome, when the auto rung applied.
    pub disposition: SelectionDisposition,
}

impl fmt::Display for SelectionDisposition {
    /// Render what the optional model rung did, or did not do.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotApplicable => "no model was asked to choose who answers",
            Self::Selected => "a model chose who answers",
            Self::Disabled => "choosing who answers by model is turned off here",
            Self::Unavailable => {
                "no model was available to choose, so this desk's first agent answered"
            }
            Self::InvalidOutput => {
                "the model did not name one agent, so this desk's first agent answered"
            }
        })
    }
}

/// A pure plan that either decides immediately or requests one selector call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponderPlan {
    /// No runtime selection is needed.
    Decided {
        /// The sole responder decision.
        decision: ResponderDecision,
    },
    /// Invoke a selector once, falling back deterministically.
    Select {
        /// The bounded request visible to the selector.
        request: SelectionRequest,
        /// First-candidate fallback for unavailable or invalid selection.
        fallback: ResponderDecision,
    },
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
