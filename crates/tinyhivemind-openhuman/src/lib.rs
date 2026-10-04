//! Permanent hive tools for host-supplied `OpenHuman` agents.
//!
//! Build agents on one runtime, then hand their existing handles to
//! [`OpenHumanHost`]. Their configuration and continuing sessions stay owned by
//! `OpenHuman`; the coordinator owns attributed messaging and scheduling.
//! With [`OpenHumanHost::with_hive_memory`], every seat registered afterwards
//! shares one [`HiveMemory`]: its own memory agent id under the hive's root.
//! ```no_run
//! # fn example(agent: openhuman_embed::Agent, coordinator: tinyhivemind_hives::Coordinator) -> tinyhivemind_openhuman::Result<()> {
//! let host = tinyhivemind_openhuman::OpenHumanHost::new(agent.runtime_id().into(), coordinator)?;
//! host.register_agent(agent)?;
//! # Ok(()) }
//! ```
mod error;
mod host;
pub mod journal;
mod memory;
#[cfg(any(test, feature = "offline"))]
pub mod offline;
mod tools;
pub use error::{Error, Result};
pub use host::{
    AgentFactory, AgentFuture, HostedTurn, ManagementAuthorizer, ManagementRequest, OpenHumanHost,
    RegisteredAgent, TURN_TIMEOUT, TurnHooks, TurnProgressSink,
};
pub use journal::MemoryLog;
pub use memory::{HiveMemory, HiveMemoryStore};
