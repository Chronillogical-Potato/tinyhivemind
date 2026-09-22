//! The scripted model's two dialects and its metrics, without a session.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;

use serde_json::json;
use wiremock::http::{HeaderMap, Method, Url};
use wiremock::{Request, Respond};

use super::{COMPLETION, Dialect, Metrics, ScriptedModel, dialect};

fn request(body: &serde_json::Value) -> Request {
    Request {
        url: Url::parse("http://127.0.0.1/v1/chat/completions").expect("a url"),
        method: Method::POST,
        headers: HeaderMap::new(),
        body: body.to_string().into_bytes(),
    }
}

#[test]
fn the_dialect_is_read_from_the_tools_the_request_advertises() {
    assert_eq!(
        dialect(&json!({"tools": [{"function": {"name": "complete_episode"}}]})),
        Dialect::Native
    );
    assert_eq!(
        dialect(&json!({"tools": [{"name": "mcp_call_tool"}]})),
        Dialect::Mcp
    );
    assert_eq!(dialect(&json!({"tools": []})), Dialect::None);
    assert_eq!(dialect(&json!({})), Dialect::None);
}

#[test]
fn a_call_is_emitted_once_and_receipted_once() {
    let metrics = Arc::new(Metrics::default());
    let model = ScriptedModel {
        chat: "engineering".into(),
        metrics: Arc::clone(&metrics),
    };
    // The first request advertises the belt: a call goes out.
    let _ = model.respond(&request(&json!({
        "messages": [{"role": "user", "content": "go"}],
        "tools": [{"function": {"name": "complete_episode"}}]
    })));
    let seen = metrics.snapshot();
    assert_eq!(seen.requests, 1);
    assert!(seen.bytes > 0);
    assert!(seen.round_trips.is_empty(), "nothing receipted yet");
    // The second carries the receipt: the round trip closes, no call.
    let _ = model.respond(&request(&json!({
        "messages": [{"role": "tool", "content": "recorded"}],
        "tools": [{"function": {"name": "complete_episode"}}]
    })));
    let seen = metrics.snapshot();
    assert_eq!(seen.requests, 2);
    assert_eq!(seen.round_trips.len(), 1);
    // A request with no belt gets a plain sentence and no call.
    let _ = model.respond(&request(&json!({"messages": []})));
    assert_eq!(metrics.snapshot().round_trips.len(), 1);
    metrics.reset();
    assert_eq!(metrics.snapshot().requests, 0);
    assert!(!COMPLETION.is_empty());
}

#[test]
fn the_offline_config_reaches_out_to_nothing() {
    let config = super::config();
    assert!(!config.local_ai.runtime_enabled);
    assert!(!config.runtime_python.enabled);
    assert!(config.memory_tree.embedding_endpoint.is_none());
}

#[tokio::test]
async fn the_backend_stub_says_yes() {
    let backend = super::backend().await;
    assert!(backend.uri().starts_with("http://127.0.0.1"));
}
