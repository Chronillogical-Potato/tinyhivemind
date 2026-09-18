//! Host-neutral integration surface for `TinyHiveMind`.
//!
//! This crate tells an embedding host which conversation a turn belongs to and
//! composes semantic routing with deterministic eligibility and fallback. It
//! can bind accepted route ids to already-instantiated host agents, but does
//! not construct those agents, store messages, open sessions, or call a
//! provider.
//!
//! ```
//! use tinyhivemind_embed::{ConversationKind, ConversationRef};
//!
//! let conversation = ConversationRef {
//!     id: "desk-engineering".into(),
//!     kind: ConversationKind::Desk,
//!     thread_root: None,
//! };
//! assert!(conversation.may_open_hive());
//! ```

mod error;

pub mod agents;
pub mod conversation;
pub mod routing;

pub use agents::{AgentRegistry, AgentRegistryError, RoutedAgent, RoutedAgents};
pub use conversation::{ConversationKind, ConversationRef, MessageRoute};
pub use error::{Error, Result};
pub use routing::{
    CONCURRENT_CHOICE_THRESHOLD_PARTS, CandidateProbability, ContributionProbability,
    EvaluationDisposition, RouteCandidate, Router, RouterError, RouterFuture, RoutingEvaluation,
    RoutingFallback, RoutingPlan, RoutingPolicy, RoutingRequest, route_message,
};
