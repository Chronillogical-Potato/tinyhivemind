//! A scripted OpenAI-compatible model, so a runner is proven offline.
//!
//! Also here: the runtime config an offline run needs ([`config`]) and the
//! stub for the backend calls the core makes beside inference ([`backend`]),
//! since signed out of the real one those hang rather than fail.
//!
//! A canned completion cannot exercise a transport, so an offline run proves
//! mechanics, not the task: the model answers every seat with one
//! `complete_episode` call, and the run asserts that the call became a desk
//! row. The script speaks both dialects a runner can offer it, told apart by
//! the tools the request advertises: a raw session advertises the room's
//! tools themselves, so the call is native; an embed agent advertises the
//! three MCP dispatchers, so the call is `mcp_call_tool` against the
//! `episode` server. Either way the second request carries the receipt and
//! gets a closing sentence.
//!
//! The script also keeps [`Metrics`]: every request's size, and the time from
//! emitting a tool call to seeing its receipt -- the whole harness's round
//! trip as the model experiences it, whichever road the call took. That is
//! what the example's bench compares between the runners.

mod log;
#[cfg(test)]
mod test;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub use log::{MemoryLog, Row};
use openhuman_embed::RuntimeConfig;
use serde_json::{Value, json};
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The model id the scripted endpoint answers as.
pub const MODEL: &str = "openhuman-raw-proof-model";

/// The message every scripted seat completes with.
pub const COMPLETION: &str = "offline proof: read the desk, nothing to add";

/// What the scripted model saw: the harness cost, from the model's side.
#[derive(Debug, Default)]
pub struct Metrics {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    requests: u64,
    bytes: u64,
    /// Tool calls emitted and not yet receipted, oldest first.
    pending: VecDeque<Instant>,
    round_trips: Vec<Duration>,
}

/// A snapshot of [`Metrics`].
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    /// Model requests made.
    pub requests: u64,
    /// Request bytes sent to the model, all requests.
    pub bytes: u64,
    /// Time from a tool call to its receipt, one per receipted call.
    pub round_trips: Vec<Duration>,
}

impl Metrics {
    /// Forget everything, at the start of an arm.
    pub fn reset(&self) {
        *self.inner.lock().unwrap_or_else(PoisonError::into_inner) = Inner::default();
    }

    /// What has been seen since the last reset.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        Snapshot {
            requests: inner.requests,
            bytes: inner.bytes,
            round_trips: inner.round_trips.clone(),
        }
    }

    fn saw_request(&self, bytes: usize, receipted: bool) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.requests += 1;
        inner.bytes += bytes as u64;
        if receipted && let Some(emitted) = inner.pending.pop_front() {
            inner.round_trips.push(emitted.elapsed());
        }
    }

    fn emitted_call(&self) {
        self.inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .push_back(Instant::now());
    }
}

/// Which way the request lets the model call the room's tools.
#[derive(Debug, PartialEq, Eq)]
enum Dialect {
    /// The room's tools are the request's own: call `complete_episode`.
    Native,
    /// `OpenHuman`'s dispatchers are: call `mcp_call_tool` on `episode`.
    Mcp,
    /// No tool at all: a session with no belt, such as an answer to an ask.
    None,
}

fn dialect(body: &Value) -> Dialect {
    let names: Vec<&str> = body["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    tool["function"]["name"]
                        .as_str()
                        .or_else(|| tool["name"].as_str())
                })
                .collect()
        })
        .unwrap_or_default();
    if names.contains(&"complete_episode") {
        Dialect::Native
    } else if names.contains(&"mcp_call_tool") {
        Dialect::Mcp
    } else {
        Dialect::None
    }
}

struct ScriptedModel {
    chat: String,
    metrics: Arc<Metrics>,
}

impl Respond for ScriptedModel {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let receipt = body["messages"].as_array().and_then(|messages| {
            messages
                .iter()
                .find(|m| m["role"] == "tool")
                .and_then(|m| m["content"].as_str().map(str::to_owned))
        });
        self.metrics
            .saw_request(request.body.len(), receipt.is_some());
        let arguments = json!({
            "message": COMPLETION,
            "chat": self.chat,
            "parent": null
        });
        let call = if receipt.is_some() {
            None
        } else {
            match dialect(&body) {
                Dialect::Native => Some(("complete_episode", arguments)),
                Dialect::Mcp => Some((
                    "mcp_call_tool",
                    json!({
                        "server": "episode",
                        "tool": "complete_episode",
                        "arguments": arguments
                    }),
                )),
                Dialect::None => None,
            }
        };
        let message = match &call {
            Some((name, arguments)) => {
                self.metrics.emitted_call();
                json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_complete_1",
                        "type": "function",
                        "function": { "name": name, "arguments": arguments.to_string() }
                    }]
                })
            }
            None => json!({"role": "assistant", "content": "Recorded."}),
        };
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-raw-proof",
            "object": "chat.completion",
            "created": 1_700_000_000_u64,
            "model": MODEL,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": if call.is_some() { "tool_calls" } else { "stop" }
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16}
        }))
    }
}

/// The scripted model, bound on loopback, completing into `chat` and
/// reporting into `metrics`.
pub async fn model(chat: &str, metrics: Arc<Metrics>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ScriptedModel {
            chat: chat.to_owned(),
            metrics,
        })
        .mount(&server)
        .await;
    server
}

/// The runtime config an offline run boots with: nothing that would reach
/// out or spawn. Live, a host loads its own; offline, this is the base every
/// seat is built from.
#[must_use]
pub fn config() -> RuntimeConfig {
    let mut config = RuntimeConfig::default();
    config.local_ai.runtime_enabled = false;
    config.runtime_python.enabled = false;
    config.memory_tree.spacy_enabled = false;
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;
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
