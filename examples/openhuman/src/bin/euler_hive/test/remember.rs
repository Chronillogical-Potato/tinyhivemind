//! Verified publication goes through the current turn's successful check gate.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::*;

#[tokio::test]
async fn verified_publication_requires_a_check_in_the_current_turn() {
    let dir = tempfile::tempdir().expect("dir");
    let package_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/tinyhivemind-lang/examples/hives/euler-hive");
    let documents = crate::package::read_package(&package_dir).expect("package");
    let (_, hive) = crate::package::load(documents, "test problem").expect("lowered");
    let seat = hive
        .seats
        .iter()
        .find(|seat| seat.id == "verify-constraints")
        .expect("verify seat");
    let binding = crate::memory::scope_binding(
        seat.memory.as_ref().expect("binding"),
        &hive.memory.root,
        "test",
    )
    .expect("scope");
    let engine = Arc::new(tinymemory_api::conformance::reference::ReferenceEngine::new());
    let memory = Arc::new(SeatMemory::new(
        engine,
        binding,
        Some("team:euler-hive/ws:test/project:verified".into()),
        true,
    ));
    let services = Arc::new(Services {
        search: None,
        sandbox: PythonSandbox::from_env(),
        budget: Arc::new(Budget::new(crate::budget::Limits::SMOKE)),
        run: Arc::new(RunDir::create(dir.path().join("run")).expect("run")),
        run_id: "test".into(),
        literals: vec![],
    });
    let scope = SeatScope {
        seat: seat.id.clone(),
        desk: "verify".into(),
        colleagues: vec![],
        linked_desks: vec![],
        conversation: "test".into(),
    };
    let turn = Arc::new(TurnLedger::default());
    let make_tool = |turn: &Arc<TurnLedger>| {
        belt(
            &BTreeSet::from([SeatTool::Remember]),
            &scope,
            turn,
            &services,
            &memory,
        )
        .remove(0)
    };
    let tool = make_tool(&turn);
    let verified = json!({"kind": "outcome", "text": "a checked finding", "verified": true});
    let refused = tool.execute(verified.clone()).await.expect("tool result");
    assert!(
        refused.is_error,
        "a writable pool does not prove verification"
    );
    assert_eq!(turn.memory().1, 0, "a refused finding is not published");
    turn.record("run_python", "failed check".into(), false);
    assert!(
        tool.execute(verified.clone())
            .await
            .expect("result")
            .is_error
    );
    turn.record("run_python", "successful check".into(), true);
    assert!(
        !tool
            .execute(verified.clone())
            .await
            .expect("result")
            .is_error
    );
    assert_eq!(turn.memory().1, 1);
    let fresh = Arc::new(TurnLedger::default());
    assert!(
        make_tool(&fresh)
            .execute(verified)
            .await
            .expect("result")
            .is_error,
        "checks from prior turns cannot authorize publication"
    );
    assert!(
        !make_tool(&fresh)
            .execute(json!({"kind":"note", "text":"an unverified local note"}))
            .await
            .expect("result")
            .is_error
    );
}
