//! Replaceable transactional snapshot persistence.
#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(test)]
mod test;
mod types;
use crate::{Error, Result};
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStorage;
use std::sync::Mutex;
pub use types::{AgentRecord, Delivery, DeliveryStatus, EpisodeRecord, RunningTurn, StoredState};

/// Atomic snapshot persistence with revision compare-and-swap.
pub trait Storage: Send + Sync {
    /// Load the committed snapshot.
    /// # Errors
    /// Returns storage or snapshot decoding errors.
    fn load(&self) -> Result<StoredState>;
    /// Replace the snapshot only when its revision equals `expected_revision`.
    /// # Errors
    /// Returns revision conflicts, invalid next revisions, or persistence errors.
    fn commit(&self, expected_revision: u64, next: &StoredState) -> Result<()>;
}
/// In-process implementation of the same transactional storage contract.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    state: Mutex<StoredState>,
}
impl MemoryStorage {
    /// Create an empty store at revision zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}
impl Storage for MemoryStorage {
    fn load(&self) -> Result<StoredState> {
        Ok(self.state.lock().map_err(|_| Error::Poisoned)?.clone())
    }
    fn commit(&self, expected_revision: u64, next: &StoredState) -> Result<()> {
        let mut current = self.state.lock().map_err(|_| Error::Poisoned)?;
        validate_revision(current.revision, expected_revision, next.revision)?;
        *current = next.clone();
        Ok(())
    }
}
fn validate_revision(actual: u64, expected: u64, next: u64) -> Result<()> {
    if actual != expected {
        return Err(Error::RevisionConflict { expected, actual });
    }
    if expected.checked_add(1) != Some(next) {
        return Err(Error::InvalidRevision);
    }
    Ok(())
}
