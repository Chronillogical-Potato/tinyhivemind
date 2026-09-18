//! First-class `OpenHuman` bindings for one host-owned `TinyHiveMind` desk.
//!
//! This crate owns the immutable relationship between canonical hive ids and
//! already-instantiated [`openhuman_embed::Agent`] handles. It proposes work
//! and folds host-committed utterances into caller-owned completion state; it
//! never stores a transcript, appends a row, or retains an `OpenHuman` session
//! id. The host creates the runtime and agents, executes proposed turns,
//! durably assigns sequences, and feeds those committed events back through
//! [`CompletionDriver`].
//!
//! This adapter is intentionally the OpenHuman-specific workspace boundary.
//! Its direct `openhuman-embed` dependency sets the root workspace Rust floor;
//! the pure `tinyhivemind-core` and `tinyhivemind-hive` dependency graphs stay
//! separate and continue to be checked as pure crates.
//!
//! ```
//! use openhuman_embed::{AgentSpec, Runtime, Workspace};
//! use tinyhivemind::{Conversation, Sequence, desk::{Desk, ResponderMode}};
//! use tinyhivemind_embed::RouteCandidate;
//! use tinyhivemind_hive::CompletionEpisodeState;
//! use tinyhivemind_openhuman::{
//!     AgentBinding, CompletionDriver, HiveGraph, OpenHumanHive,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let executor = tokio::runtime::Builder::new_current_thread()
//!     .enable_all()
//!     .build()?;
//! let runtime = executor.block_on(
//!     Runtime::builder()
//!         .workspace(Workspace::Ephemeral)
//!         .api_key("th_example_tinyhivemind")
//!         .build(),
//! )?;
//! let agent = runtime.agent(AgentSpec::new("runtime-solver"))?;
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
//! let hive = OpenHumanHive::new(graph, vec![AgentBinding::new("solver", agent)])?;
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
//! assert_eq!(driver.pending_round(&state)?.agents()[0].hive_agent_id, "solver");
//! # Ok(())
//! # }
//! ```

pub mod driver;
pub mod error;
pub mod graph;

#[cfg(test)]
mod test_support;

pub use driver::{
    BroadcastRouting, CommittedUtterance, CompletionDriver, DriverState, HostAction, PendingAgent,
    PendingRound, Transition,
};
pub use error::{Error, Result};
pub use graph::{AgentBinding, HiveGraph, OpenHumanHive};
