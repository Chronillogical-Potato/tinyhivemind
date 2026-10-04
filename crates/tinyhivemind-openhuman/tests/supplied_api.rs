//! Public runtime binding regression.
// Test assertions deliberately panic on invalid fixture construction.
#![allow(clippy::unwrap_used)]
use std::sync::Arc;
use tinyhivemind_hives::{Coordinator, CoordinatorOptions, MemoryStorage};
use tinyhivemind_openhuman::OpenHumanHost;
#[test]
fn rejects_an_unrelated_coordinator_runtime() {
    let coordinator = Coordinator::new(
        "runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions::default(),
    )
    .unwrap();
    assert!(OpenHumanHost::new("other".into(), coordinator).is_err());
}
