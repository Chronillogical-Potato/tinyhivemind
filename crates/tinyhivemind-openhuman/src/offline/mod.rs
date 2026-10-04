//! Offline host configuration and incidental backend fixtures.
//! The example owns its scripted model; this module makes host construction
//! deterministic without changing the adapter's supplied-agent boundary.
/// In-memory host log for standalone research examples.
pub use crate::journal::{MemoryLog, Row};
use openhuman_embed::RuntimeConfig;
use serde_json::json;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};
#[cfg(test)]
mod test;
/// The runtime config an offline run boots with: nothing that would reach
/// out or spawn. Live, a host loads its own; offline, this is the base every
/// seat is built from.
#[must_use]
pub fn config() -> RuntimeConfig {
    let mut config = RuntimeConfig::default();
    // Offline providers use native structured calls, not prose dispatch.
    config.agent.tool_dispatcher = "auto".into();
    config.local_ai.runtime_enabled = false;
    config.runtime_python.enabled = false;
    // The memory lifecycle recalls a pack before every turn and logs every
    // turn after it; offline there is no engine to answer either.
    config.memory.recall.enabled = false;
    config.memory.conversations.enabled = false;
    config
}

/// A backend that says yes to everything. The core makes non-inference
/// calls to it; signed out of the real one those hang rather than fail.
pub async fn backend() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {"id": "offline", "email": "local@openhuman.local"}
        })))
        .mount(&server)
        .await;
    server
}
