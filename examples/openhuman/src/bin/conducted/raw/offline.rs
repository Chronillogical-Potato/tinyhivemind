//! A scripted OpenAI-compatible model, so the raw runner is proven offline.
//!
//! The embed runner only runs live: its tools reach the agent through MCP,
//! and a canned completion cannot exercise a transport. A raw session's tools
//! are in-process, so a model that answers with one native tool call drives
//! the whole path -- belt, gate, record, driver -- with no credential and no
//! network.
//!
//! Two moves, told apart by what the request carries: a request with no tool
//! result yet gets one `complete_episode` call naming the desk's chat; one
//! that carries the receipt gets a closing sentence.

use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The model id the scripted endpoint answers as.
pub const MODEL: &str = "openhuman-raw-proof-model";

/// The message every scripted seat completes with.
pub const COMPLETION: &str = "offline proof: read the desk, nothing to add";

struct ScriptedModel {
    chat: String,
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
        if let Some(text) = &receipt {
            eprintln!("[model] tool receipt: {text}");
        }
        let message = if receipt.is_some() {
            json!({"role": "assistant", "content": "Recorded."})
        } else {
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_complete_1",
                    "type": "function",
                    "function": {
                        "name": "complete_episode",
                        "arguments": json!({
                            "message": COMPLETION,
                            "chat": self.chat,
                            "parent": null
                        }).to_string()
                    }
                }]
            })
        };
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-raw-proof",
            "object": "chat.completion",
            "created": 1_700_000_000_u64,
            "model": MODEL,
            "choices": [{
                "index": 0,
                "message": message,
                "finish_reason": if receipt.is_some() { "stop" } else { "tool_calls" }
            }],
            "usage": {"prompt_tokens": 12, "completion_tokens": 4, "total_tokens": 16}
        }))
    }
}

/// The scripted model, bound on loopback, completing into `chat`.
pub async fn model(chat: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ScriptedModel {
            chat: chat.to_owned(),
        })
        .mount(&server)
        .await;
    server
}
