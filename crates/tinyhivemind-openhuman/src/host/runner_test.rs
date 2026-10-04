//! Request attribution and failure mapping.
// Test assertions deliberately panic on invalid fixture construction.
#![allow(clippy::unwrap_used)]
use super::*;
#[test]
fn renders_attribution_and_explicit_episode_without_reseeding() {
    let request = TurnRequest {
        agent_id: "a".into(),
        session_id: Some("history".into()),
        messages: vec![],
        memberships: vec![],
        episode: None,
    };
    let prompt = render(&request).unwrap();
    assert!(prompt.contains("\"agent_id\":\"a\""));
    assert!(prompt.contains("\"session_id\":\"history\""));
    assert!(
        map_error(&Error::TimedOut)
            .to_string()
            .contains("timed out")
    );
}
