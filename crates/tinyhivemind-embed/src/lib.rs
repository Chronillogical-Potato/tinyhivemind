//! Host-neutral integration surface for `TinyHiveMind`.
//!
//! This crate tells an embedding host which conversation a turn belongs to and
//! composes semantic routing with deterministic eligibility and fallback. It
//! does not parse host ids, store messages, open sessions, or call a provider.
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

pub mod conversation;
pub mod routing;

pub use conversation::{ConversationKind, ConversationRef, MessageRoute, MessageRouteError};
pub use routing::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterError, RouterFuture, RoutingEvaluation, RoutingFallback, RoutingPlan, RoutingPolicy,
    RoutingRequest, route_message,
};
