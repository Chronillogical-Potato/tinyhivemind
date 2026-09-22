//! The completion driver: who runs next in a completion episode, what a
//! committed row means, and what a seat is told before a turn.
//!
//! This crate owns the immutable relationship between canonical hive ids and
//! the handles a host binds to them -- any [`BoundAgent`] the host chooses,
//! since the driver stores a handle and hands it back with a pending round
//! and never runs one. It proposes work and folds host-committed utterances
//! into caller-owned completion state; it never stores a transcript, appends
//! a row, or retains a session id. The host creates the runtime and the
//! seats, executes proposed turns, durably assigns sequences, and feeds those
//! committed events back through [`CompletionDriver`].
//!
//! It names no harness. `tinyhivemind-openhuman` is the `OpenHuman` adapter:
//! it binds `OpenHuman`'s two kinds of seat and runs their turns, and it is the
//! one crate in the workspace that links one. A host that runs seats some
//! other way implements [`BoundAgent`] itself, which is one method.
//!
//! ```
//! use tinyhivemind::{Conversation, Sequence, desk::{Desk, ResponderMode}};
//! use tinyhivemind_driver::{AgentBinding, BoundAgent, BoundHive, CompletionDriver, HiveGraph};
//! use tinyhivemind_embed::RouteCandidate;
//! use tinyhivemind_hive::CompletionEpisodeState;
//!
//! // The host's handle for a seat: here, a name and nothing behind it.
//! #[derive(Clone, Debug)]
//! struct Seat(&'static str);
//!
//! impl BoundAgent for Seat {
//!     fn runtime_id(&self) -> &str {
//!         self.0
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let graph = HiveGraph::new(
//!     Desk {
//!         id: "engineering".into(),
//!         name: "Engineering".into(),
//!         description: None,
//!         members: vec!["solver".into()],
//!         responder_mode: ResponderMode::Auto,
//!     },
//!     vec![RouteCandidate {
//!         id: "solver".into(),
//!         label: "Solver".into(),
//!         role: None,
//!         description: None,
//!         capabilities: Vec::new(),
//!         learned_topics: Vec::new(),
//!         available: true,
//!     }],
//! );
//! let hive = BoundHive::new(graph, vec![AgentBinding::new("solver", Seat("runtime-solver"))])?;
//! let episode = CompletionEpisodeState::opened(
//!     Conversation {
//!         desk_id: "engineering".into(),
//!         desk_name: "Engineering".into(),
//!         thread_root: None,
//!     },
//!     Sequence(0),
//!     ["solver"],
//! )?;
//! let driver = CompletionDriver::new(&hive, 1)?;
//! let state = driver.start(episode)?;
//! let round = driver.pending_round(&state)?;
//! assert_eq!(round.agents()[0].hive_agent_id, "solver");
//! assert_eq!(round.agents()[0].agent.runtime_id(), "runtime-solver");
//! # Ok(())
//! # }
//! ```

pub mod driver;
pub mod error;
pub mod graph;

#[cfg(test)]
mod test_support;

pub use driver::{
    AssignmentSpend, BroadcastRouting, Channel, CommittedUtterance, CompletionDriver,
    ConversationView, DriverState, EpisodeBrief, Handoff, HostAction, Ledger, PendingAgent,
    PendingRound, Seen, Transition, standing_contract,
};
pub use error::{Error, Result};
pub use graph::{AgentBinding, BoundAgent, BoundHive, HiveGraph};
