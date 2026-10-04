//! Single-row versioned SQLite snapshot with transactional revision checks.
use super::{Storage, StoredState, validate_revision};
use crate::{Error, Result};
use rusqlite::{Connection, TransactionBehavior, params};
use std::{path::Path, sync::Mutex};
/// SQLite-backed durable storage, enabled by the default `sqlite` feature.
#[derive(Debug)]
pub struct SqliteStorage {
    connection: Mutex<Connection>,
}
impl SqliteStorage {
    /// Open or initialize a version-one snapshot database.
    /// # Errors
    /// Returns SQLite errors or rejects an unsupported schema version.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS hivemind_snapshot (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL, revision TEXT NOT NULL, state TEXT NOT NULL);")?;
        connection.execute(
            "INSERT OR IGNORE INTO hivemind_snapshot VALUES (1, 1, 0, ?1)",
            [serde_json::to_string(&StoredState::default())?],
        )?;
        let version: i64 = connection.query_row(
            "SELECT schema_version FROM hivemind_snapshot WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        if version != 1 {
            return Err(Error::InvalidState(format!(
                "unsupported SQLite schema version {version}"
            )));
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}
impl Storage for SqliteStorage {
    fn load(&self) -> Result<StoredState> {
        let connection = self.connection.lock().map_err(|_| Error::Poisoned)?;
        let (revision, json): (String, String) = connection.query_row(
            "SELECT revision, state FROM hivemind_snapshot WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let state: StoredState = serde_json::from_str(&json)?;
        if state.revision.to_string() != revision {
            return Err(Error::InvalidState(
                "SQLite revision differs from snapshot".into(),
            ));
        }
        Ok(state)
    }
    fn commit(&self, expected_revision: u64, next: &StoredState) -> Result<()> {
        let mut connection = self.connection.lock().map_err(|_| Error::Poisoned)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision: String = transaction.query_row(
            "SELECT revision FROM hivemind_snapshot WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        let revision = revision
            .parse()
            .map_err(|_| Error::InvalidState("invalid SQLite revision".into()))?;
        validate_revision(revision, expected_revision, next.revision)?;
        transaction.execute(
            "UPDATE hivemind_snapshot SET revision=?1, state=?2 WHERE id=1",
            params![next.revision.to_string(), serde_json::to_string(next)?],
        )?;
        transaction.commit()?;
        Ok(())
    }
}
