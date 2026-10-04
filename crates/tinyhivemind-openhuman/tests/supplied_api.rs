//! Public runtime binding regression.
// Test assertions deliberately panic on invalid fixture construction.
#![allow(clippy::unwrap_used)]
use std::sync::Arc;
use tinyhivemind_hives::{Coordinator, CoordinatorOptions, MemoryStorage};
use tinyhivemind_openhuman::OpenHumanHost;
#[tokio::test]
async fn rejects_an_unrelated_coordinator_runtime() {
    let coordinator = Coordinator::new(
        "runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    assert!(OpenHumanHost::new("other".into(), coordinator).is_err());
}
#[tokio::test]
async fn hive_memory_is_configured_before_registration_through_public_apis() {
    use tinyhivemind_openhuman::{Error, HiveMemory};
    let coordinator = Coordinator::new(
        "runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    let memory = HiveMemory::for_hive("run-42").unwrap();
    let host = OpenHumanHost::new("runtime".into(), coordinator)
        .unwrap()
        .with_hive_memory(memory.clone())
        .unwrap();
    assert_eq!(host.hive_memory(), Some(&memory));
    assert_eq!(
        memory.binding("scout").unwrap().root_namespace(),
        Some("team:run-42")
    );
    assert!(matches!(
        HiveMemory::for_hive("run 42"),
        Err(Error::InvalidMemoryRoot { .. })
    ));
}
