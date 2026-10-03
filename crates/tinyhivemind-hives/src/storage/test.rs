//! Shared storage revision and atomicity contract.
// Panicking assertions are confined to deterministic test fixtures.
#![allow(clippy::unwrap_used, clippy::panic)]
use super::*;
#[test]
fn memory_commits_exactly_one_revision_and_rejects_stale_writers() {
    let storage = MemoryStorage::new();
    let mut state = storage.load().unwrap();
    assert_eq!(state.revision, 0);
    state.revision = 1;
    storage.commit(0, &state).unwrap();
    assert!(storage.commit(0, &state).is_err());
    assert_eq!(storage.load().unwrap().revision, 1);
    state.revision = 3;
    assert!(storage.commit(1, &state).is_err());
    assert_eq!(storage.load().unwrap().revision, 1);
}
fn contract(storage: &dyn Storage) {
    let mut state = storage.load().unwrap();
    let start = state.revision;
    state.revision += 1;
    state.agents.insert(
        "a".into(),
        AgentRecord {
            session_id: Some("session".into()),
            parked: false,
        },
    );
    storage.commit(start, &state).unwrap();
    assert!(matches!(
        storage.commit(start, &state),
        Err(Error::RevisionConflict { .. })
    ));
    let committed = storage.load().unwrap();
    assert_eq!(committed.agents["a"].session_id.as_deref(), Some("session"));
    state.revision += 2;
    assert!(matches!(
        storage.commit(committed.revision, &state),
        Err(Error::InvalidRevision)
    ));
    assert_eq!(storage.load().unwrap().revision, committed.revision);
}
#[test]
fn memory_obeys_shared_atomic_contract() {
    contract(&MemoryStorage::new());
}
#[cfg(feature = "sqlite")]
#[test]
fn sqlite_obeys_shared_contract_and_reopens_durable_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hives.sqlite");
    let store = SqliteStorage::open(&path).unwrap();
    contract(&store);
    drop(store);
    let reopened = SqliteStorage::open(&path).unwrap();
    assert_eq!(
        reopened.load().unwrap().agents["a"].session_id.as_deref(),
        Some("session")
    );
    let mut state = reopened.load().unwrap();
    let original = state.revision;
    state.revision += 1;
    let competitor = SqliteStorage::open(&path).unwrap();
    competitor.commit(original, &state).unwrap();
    assert!(matches!(
        reopened.commit(original, &state),
        Err(Error::RevisionConflict { .. })
    ));
}
#[cfg(feature = "sqlite")]
#[test]
fn sqlite_rejects_invalid_schema_and_revision_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bad.sqlite");
    let store = SqliteStorage::open(&path).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE hivemind_snapshot SET schema_version=2", [])
        .unwrap();
    assert!(matches!(
        SqliteStorage::open(&path),
        Err(Error::InvalidState(_))
    ));
    connection
        .execute(
            "UPDATE hivemind_snapshot SET schema_version=1, revision='invalid'",
            [],
        )
        .unwrap();
    let store = SqliteStorage::open(&path).unwrap();
    assert!(store.load().is_err());
    assert!(
        store
            .commit(
                0,
                &StoredState {
                    revision: 1,
                    ..StoredState::default()
                }
            )
            .is_err()
    );
}
