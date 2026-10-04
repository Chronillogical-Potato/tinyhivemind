//! Offline configuration and loopback backend contract.
#[test]
fn the_offline_config_reaches_out_to_nothing() {
    let config = super::config();
    assert!(!config.local_ai.runtime_enabled);
    assert!(!config.runtime_python.enabled);
    assert!(!config.memory.recall.enabled);
    assert!(!config.memory.conversations.enabled);
}

/// The dialect the scripted route is written in, pinned.
///
/// `tool_dispatcher` defaulted to `"auto"` upstream until the pin bump that
/// follows `main` changed it to `"python"`, which asks the model for a tool
/// call in prose and parses it back. The scripted model answers in the
/// native dialect, so under `"python"` nothing it said was ever read as a
/// call: every offline run recorded zero calls and retried until it gave up,
/// and the symptom pointed at this crate rather than at a default. Naming it
/// here costs one assertion; finding it cost a bisect over 1528 commits.
#[test]
fn the_offline_config_pins_the_native_tool_dialect() {
    assert_eq!(super::config().agent.tool_dispatcher, "auto");
}

#[tokio::test]
async fn the_backend_stub_says_yes() {
    let backend = super::backend().await;
    assert!(backend.uri().starts_with("http://127.0.0.1"));
}
