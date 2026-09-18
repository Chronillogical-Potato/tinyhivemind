//! Stable semantic-routing payloads and the provider-neutral router port.

use std::{error::Error as StdError, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use tinyhivemind::responder::Probability;

use crate::ConversationRef;

/// Why semantic routing is being requested.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoutingSource {
    /// An operator or person authored an unaddressed desk message.
    DeskMessage,
    /// An agent called `broadcast` to hand work to the best-placed teammates.
    AgentBroadcast {
        /// Canonical id of the agent handing off the work.
        author_id: String,
    },
}

/// One candidate visible to semantic routing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RouteCandidate {
    /// Canonical agent id.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Current organizational role.
    pub role: Option<String>,
    /// Short description of the agent's remit.
    pub description: Option<String>,
    /// Explicit capability labels.
    pub capabilities: Vec<String>,
    /// Topics learned from prior attributed work.
    pub learned_topics: Vec<String>,
    /// Whether the host currently permits an immediate turn.
    pub available: bool,
}

/// Frozen, host-calibrated routing thresholds and bounds.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingPolicy {
    /// Minimum concentration accepted for the primary Choice.
    pub minimum_confidence: Probability,
    /// Higher confidence required when the request is high impact.
    pub high_impact_minimum_confidence: Probability,
    /// Probability at which missing routing information requires escalation.
    pub clarification_threshold: Probability,
    /// Probability at which the high-impact confidence rule applies.
    pub high_impact_threshold: Probability,
    /// Maximum number of agents in the opening round, including the primary.
    pub round_width: usize,
    /// Maximum alternatives in one provider Choice, including `none`.
    pub choice_option_limit: usize,
}

/// Complete semantic-routing input for one message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingRequest {
    /// Exact authored message.
    pub message: String,
    /// Provenance and semantic intent of the message being routed.
    pub source: RoutingSource,
    /// Canonical conversation and semantic surface.
    pub conversation: ConversationRef,
    /// Desk purpose supplied by the host.
    pub desk_purpose: Option<String>,
    /// Relevant attributed thread context selected by the host.
    pub thread_context: Vec<String>,
    /// Effective desk members in deterministic desk order.
    pub candidates: Vec<RouteCandidate>,
    /// Version of the candidate snapshot.
    pub roster_version: u64,
    /// Frozen routing thresholds and bounds.
    pub policy: RoutingPolicy,
}

/// One primary Choice probability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CandidateProbability {
    /// Candidate id, or the reserved `none` alternative.
    pub candidate_id: String,
    /// Fixed-point probability.
    pub probability: Probability,
}

/// One independent candidate-contribution Noul.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ContributionProbability {
    /// Canonical candidate id.
    pub candidate_id: String,
    /// Probability that this candidate adds distinct relevant expertise.
    pub probability: Probability,
}

/// How pure acceptance treated a provider evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationDisposition {
    /// Provider output has not yet passed pure acceptance.
    Unchecked,
    /// Every deterministic acceptance rule passed.
    Accepted,
    /// A second, reasoning router should evaluate the same snapshot.
    EscalationRequired,
    /// Provider output was malformed or contradicted its declared domain.
    Rejected,
}

/// Auditable fixed-point semantic evaluation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RoutingEvaluation {
    /// Highest-probability primary alternative.
    pub primary_responder: String,
    /// Complete Choice distribution over eligible candidates plus `none`.
    pub primary_probabilities: Vec<CandidateProbability>,
    /// Choice distribution concentration.
    pub confidence: Probability,
    /// Whether one competent agent is insufficient, retained for audit.
    pub needs_collaboration: Probability,
    /// Whether essential routing information is absent.
    pub needs_clarification: Probability,
    /// Independent distinct-contribution probabilities retained for audit.
    pub contributions: Vec<ContributionProbability>,
    /// Whether errors would have unusually serious consequences.
    pub high_impact: Probability,
    /// Provider-returned model identity.
    pub model_identity: String,
    /// Version of the question schema used to produce this result.
    pub question_schema_version: u32,
    /// Candidate snapshot version seen by the provider.
    pub roster_version: u64,
    /// Pure acceptance disposition.
    pub disposition: EvaluationDisposition,
}

/// Why deterministic routing supplied the responder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingFallback {
    /// An explicit mention already named the destination.
    ExplicitMention,
    /// A direct conversation already named the destination.
    DirectConversation,
    /// This surface retains its existing single-responder rule.
    SurfaceRule,
    /// No semantic router was configured or its transport failed.
    ProviderUnavailable,
    /// Provider output failed pure validation.
    RejectedOutput,
    /// An agent broadcast had invalid provenance or included its author.
    InvalidBroadcast,
    /// The candidate snapshot changed before acceptance.
    StaleRoster,
    /// The one permitted reasoning escalation failed.
    EscalationFailed,
    /// No available eligible candidate could be routed.
    NoEligibleCandidate,
}

/// Accepted routing result for one message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RoutingPlan {
    /// Run one selected responder.
    One {
        /// Canonical responder id.
        responder_id: String,
        /// Accepted semantic evaluation.
        evaluation: RoutingEvaluation,
    },
    /// Open one bounded desk-scoped hive episode whose primary and invited
    /// agents receive the message concurrently in the opening round.
    Hive {
        /// Canonical primary responder id.
        primary_id: String,
        /// Additional specialists in contribution then desk order.
        invited_ids: Vec<String>,
        /// Accepted semantic evaluation.
        evaluation: RoutingEvaluation,
    },
    /// Ask for missing information instead of selecting silently.
    Clarify {
        /// Accepted evaluation that established the need to clarify.
        evaluation: RoutingEvaluation,
    },
    /// Use the existing deterministic destination.
    Fallback {
        /// Canonical deterministic responder id.
        responder_id: String,
        /// Why semantic routing did not decide.
        reason: RoutingFallback,
    },
}

/// Boxed provider or reasoning-router failure.
pub type RouterError = Box<dyn StdError + Send + Sync + 'static>;

/// Executor-neutral future returned by [`Router`].
pub type RouterFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RoutingEvaluation, RouterError>> + Send + 'a>>;

/// A semantic router over one immutable candidate snapshot.
pub trait Router: Send + Sync {
    /// Evaluate the request without applying deterministic acceptance policy.
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a>;
}
