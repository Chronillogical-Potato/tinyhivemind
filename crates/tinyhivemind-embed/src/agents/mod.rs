//! Binding accepted routes to caller-instantiated agent handles.
//!
//! The generic handle is intentionally opaque. An embedding host can supply
//! `openhuman_embed::Agent`, an actor handle, or another runtime-owned value;
//! `TinyHiveMind` retains the instance and resolves routing ids to references.
//! Provider sessions, transcript storage, and compaction stay with that value's
//! runtime rather than being copied into this crate.

#[cfg(test)]
mod test;
mod types;

pub use types::{AgentRegistry, AgentRegistryError, RoutedAgent, RoutedAgents};
