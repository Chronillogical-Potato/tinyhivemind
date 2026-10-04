//! SQLite reopen, competing writers, schema validation and v1 migration.
// Panicking assertions are confined to deterministic test fixtures.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::super::test::{commit, contract, row, state_at};
use super::*;
use crate::{AgentRecord, Storage};

#[tokio::test]
async fn sqlite_obeys_shared_contract_and_reopens_durable_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hives.sqlite");
    let store = SqliteStorage::open(&path).unwrap();
    contract(&store).await;
    drop(store);
    let reopened = SqliteStorage::open(&path).unwrap();
    let loaded = reopened.load().await.unwrap();
    assert_eq!(loaded.agents["a"].session_id.as_deref(), Some("session"));
    assert_eq!(loaded.messages.len(), 3);
    assert_eq!(loaded.accepted.len(), 2);
    let mut state = loaded.clone();
    let original = state.revision;
    state.revision += 1;
    let competitor = SqliteStorage::open(&path).unwrap();
    commit(&competitor, original, &state, &[]).await.unwrap();
    assert!(matches!(
        commit(&reopened, original, &state, &[]).await,
        Err(Error::RevisionConflict { .. })
    ));
}
#[tokio::test]
async fn sqlite_rejects_invalid_schema_and_revision_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.sqlite");
    drop(SqliteStorage::open(&path).unwrap());
    let connection = Connection::open(&path).unwrap();
    connection
        .execute("UPDATE hivemind_snapshot SET schema_version=3", [])
        .unwrap();
    assert!(matches!(
        SqliteStorage::open(&path),
        Err(Error::InvalidState(_))
    ));
    connection
        .execute(
            "UPDATE hivemind_snapshot SET schema_version=2, revision='invalid'",
            [],
        )
        .unwrap();
    let store = SqliteStorage::open(&path).unwrap();
    assert!(store.load().await.is_err());
    assert!(
        commit(&store, 0, &state_at(1), &[row(0, false)])
            .await
            .is_err()
    );
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM hivemind_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}
#[tokio::test]
async fn sqlite_migrates_a_version_one_snapshot_into_the_transcript_table() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("v1.sqlite");
    let mut legacy = state_at(4);
    legacy.agents.insert("a".into(), AgentRecord::default());
    legacy.append(row(0, true));
    legacy.append(row(1, false));
    // Version one wrote the whole snapshot, transcript included, in one row.
    let mut json = serde_json::to_value(&legacy).unwrap();
    json["messages"] = serde_json::to_value(&legacy.messages).unwrap();
    json["accepted"] = serde_json::to_value(&legacy.accepted).unwrap();
    // ...and predates writer fencing, so it carries no epoch.
    json.as_object_mut().unwrap().remove("writer_epoch");
    {
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE hivemind_snapshot (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL, revision TEXT NOT NULL, state TEXT NOT NULL);")
            .unwrap();
        connection
            .execute(
                "INSERT INTO hivemind_snapshot VALUES (1, 1, '4', ?1)",
                [json.to_string()],
            )
            .unwrap();
    }
    let store = SqliteStorage::open(&path).unwrap();
    let loaded = store.load().await.unwrap();
    assert_eq!(loaded.revision, 4);
    assert_eq!(loaded.writer_epoch, 0);
    assert_eq!(loaded.messages, legacy.messages);
    assert_eq!(loaded.accepted, legacy.accepted);
    assert!(loaded.agents.contains_key("a"));
    let connection = Connection::open(&path).unwrap();
    let state: String = connection
        .query_row("SELECT state FROM hivemind_snapshot", [], |r| r.get(0))
        .unwrap();
    assert!(!state.contains("\"messages\""));
    drop(store);
    assert_eq!(
        SqliteStorage::open(&path)
            .unwrap()
            .load()
            .await
            .unwrap()
            .messages
            .len(),
        2
    );
}
