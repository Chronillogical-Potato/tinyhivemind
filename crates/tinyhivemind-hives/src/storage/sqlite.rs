//! Version-two SQLite storage: one state row plus an append-only transcript
//! table, both written in one immediate transaction.
use super::{Commit, Storage, StorageFuture, StoredState, TranscriptRow, validate};
use crate::{Error, Message, Result, SendMessage};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

const SCHEMA_VERSION: i64 = 2;
/// SQLite-backed durable storage, enabled by the default `sqlite` feature.
///
/// Its operations are synchronous SQLite calls inside the returned futures:
/// short local transactions, run on whichever executor polls them.
#[derive(Debug)]
pub struct SqliteStorage {
    connection: Mutex<Connection>,
}
impl SqliteStorage {
    /// Open or initialize a version-two database, migrating a version-one
    /// whole-snapshot database by moving its transcript into rows.
    /// # Errors
    /// Returns SQLite errors or rejects an unsupported schema version.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS hivemind_snapshot (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL, revision TEXT NOT NULL, state TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS hivemind_messages (position INTEGER PRIMARY KEY, sequence TEXT NOT NULL UNIQUE, row TEXT NOT NULL);",
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO hivemind_snapshot VALUES (1, ?1, '0', ?2)",
            params![
                SCHEMA_VERSION,
                serde_json::to_string(&StoredState::default())?
            ],
        )?;
        let version: i64 = connection.query_row(
            "SELECT schema_version FROM hivemind_snapshot WHERE id=1",
            [],
            |row| row.get(0),
        )?;
        match version {
            SCHEMA_VERSION => {}
            1 => migrate_v1(&mut connection)?,
            other => {
                return Err(Error::InvalidState(format!(
                    "unsupported SQLite schema version {other}"
                )));
            }
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
}
impl Storage for SqliteStorage {
    fn load(&self) -> StorageFuture<'_, StoredState> {
        Box::pin(async move {
            let connection = self.connection.lock().map_err(|_| Error::Poisoned)?;
            let (revision, json): (String, String) = connection.query_row(
                "SELECT revision, state FROM hivemind_snapshot WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let mut state: StoredState = serde_json::from_str(&json)?;
            if state.revision.to_string() != revision {
                return Err(Error::InvalidState(
                    "SQLite revision differs from snapshot".into(),
                ));
            }
            let mut statement =
                connection.prepare("SELECT row FROM hivemind_messages ORDER BY position")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                state.append(serde_json::from_str(&row?)?);
            }
            Ok(state)
        })
    }
    fn commit<'a>(&'a self, commit: Commit<'a>) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let mut connection = self.connection.lock().map_err(|_| Error::Poisoned)?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let revision: String = transaction.query_row(
                "SELECT revision FROM hivemind_snapshot WHERE id=1",
                [],
                |row| row.get(0),
            )?;
            let revision = revision
                .parse()
                .map_err(|_| Error::InvalidState("invalid SQLite revision".into()))?;
            let last: Option<String> = transaction
                .query_row(
                    "SELECT sequence FROM hivemind_messages ORDER BY position DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let last = last
                .map(|sequence| sequence.parse())
                .transpose()
                .map_err(|_| Error::InvalidState("invalid SQLite transcript sequence".into()))?;
            validate(revision, last, &commit)?;
            transaction.execute(
                "UPDATE hivemind_snapshot SET revision=?1, state=?2 WHERE id=1",
                params![
                    commit.state.revision.to_string(),
                    serde_json::to_string(commit.state)?
                ],
            )?;
            insert_rows(&transaction, commit.appended)?;
            transaction.commit()?;
            Ok(())
        })
    }
}
fn insert_rows(transaction: &Transaction<'_>, rows: &[TranscriptRow]) -> Result<()> {
    let mut statement =
        transaction.prepare("INSERT INTO hivemind_messages (sequence, row) VALUES (?1, ?2)")?;
    for row in rows {
        statement.execute(params![
            row.message.sequence.to_string(),
            serde_json::to_string(row)?
        ])?;
    }
    Ok(())
}
/// Move a version-one snapshot's embedded transcript into transcript rows.
fn migrate_v1(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let json: String = transaction.query_row(
        "SELECT state FROM hivemind_snapshot WHERE id=1",
        [],
        |row| row.get(0),
    )?;
    let mut legacy: serde_json::Value = serde_json::from_str(&json)?;
    let messages: Vec<Message> = serde_json::from_value(legacy["messages"].take())?;
    let mut accepted: BTreeMap<String, SendMessage> =
        serde_json::from_value(legacy["accepted"].take())?;
    let rows: Vec<_> = messages
        .into_iter()
        .map(|message| TranscriptRow {
            accepted: accepted.remove(&message.message_id),
            message,
        })
        .collect();
    let state: StoredState = serde_json::from_value(legacy)?;
    insert_rows(&transaction, &rows)?;
    transaction.execute(
        "UPDATE hivemind_snapshot SET schema_version=?1, state=?2 WHERE id=1",
        params![SCHEMA_VERSION, serde_json::to_string(&state)?],
    )?;
    transaction.commit()?;
    Ok(())
}
#[cfg(test)]
#[path = "sqlite_test.rs"]
mod test;
