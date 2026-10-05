//! Durable coordination of supplied agents across dynamic hives.
//!
//! [`Coordinator`] schedules one continuing session per agent across all its
//! hives using the existing core conductor. The host supplies [`AgentRunner`]
//! handles; they never enter [`StoredState`]. [`Storage`] is a replaceable
//! async port that commits a bounded state row plus append-only transcript
//! rows, with [`MemoryStorage`] and a default-feature SQLite implementation
//! provided.
//!
//! ```
//! use std::sync::Arc;
//! use tinyhivemind_hives::{Coordinator, CoordinatorOptions, HiveInfo, MemoryStorage};
//! # fn main() -> tinyhivemind_hives::Result<()> {
//! // Executor-neutral: any executor drives the coordinator's futures.
//! futures::executor::block_on(async {
//!     let coordinator = Coordinator::new(
//!         "host-runtime".into(), Arc::new(MemoryStorage::new()),
//!         CoordinatorOptions::default(),
//!     ).await?;
//!     coordinator.create_hive(HiveInfo {
//!         hive_id: "engineering".into(), name: "Engineering".into(),
//!         description: None, members: Vec::new(),
//!     }).await?;
//!     assert_eq!(coordinator.list_hives()?.len(), 1);
//!     Ok(())
//! })
//! # }
//! ```
//!
//! Agents are instantiated and configured by the host. The coordinator holds
//! no model client or `OpenHuman` type, keeping those at the adapter boundary.
mod error;
pub use error::{Error, Result};
mod coordinator;
mod storage;
pub use coordinator::*;
pub use storage::*;
