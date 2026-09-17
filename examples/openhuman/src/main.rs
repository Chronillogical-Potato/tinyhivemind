//! Standalone proof: route one request and run the selected embedded OpenHuman seat.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use openhuman_embed::{Access, Harness, Provider, RuntimeConfig, Workspace};
use serde_json::json;
use tinyhivemind::responder::Probability;
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingPlan, RoutingPolicy, RoutingRequest,
    route_message,
};
use tinyhivemind_typesafe::{
    ChoiceAnswer, JevRouter, NoulAnswer, SystemOneAnswer, SystemOneRequest, SystemOneResponse,
    SystemOneTransport, SystemOneTransportFuture, TokenUsage,
};
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OPENHUMAN_REPLY: &str = "openhuman-seat-ok";
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Default)]
struct FixtureTransport {
    calls: AtomicUsize,
}

impl SystemOneTransport for FixtureTransport {
    /// Return the exact typed fixture after checking the router's question batch.
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            assert_eq!(request.model, "jev-latest");
            assert!(request.questions.contains_key("primary_responder"));
            assert!(request.questions.contains_key("needs_collaboration"));
            assert!(request.questions.contains_key("needs_clarification"));
            assert!(request.questions.contains_key("contributes_engineering"));
            assert!(request.questions.contains_key("contributes_legal"));
            assert!(request.questions.contains_key("high_impact"));
            Ok(fixture_response())
        })
    }
}

/// Build a high-confidence single-engineer System One response.
fn fixture_response() -> SystemOneResponse {
    SystemOneResponse {
        model: "jev-fixture".into(),
        answers: BTreeMap::from([
            (
                "primary_responder".into(),
                SystemOneAnswer::Choice(ChoiceAnswer {
                    choice: "engineering".into(),
                    probabilities: BTreeMap::from([
                        ("engineering".into(), 0.9),
                        ("legal".into(), 0.05),
                        ("none".into(), 0.05),
                    ]),
                    confidence: 0.95,
                }),
            ),
            noul("needs_collaboration", 0.1),
            noul("needs_clarification", 0.01),
            noul("contributes_engineering", 0.95),
            noul("contributes_legal", 0.2),
            noul("high_impact", 0.1),
        ]),
        usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 10,
        },
    }
}

/// Pair one Noul question id with its fixture probability.
fn noul(id: &str, probability: f64) -> (String, SystemOneAnswer) {
    (
        id.into(),
        SystemOneAnswer::Noul(NoulAnswer { noul: probability }),
    )
}

/// Build the immutable two-seat desk request routed by the proof.
fn request() -> RoutingRequest {
    RoutingRequest {
        message: "Review the launch implementation and compliance risk.".into(),
        conversation: ConversationRef {
            id: "launch".into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: Some("ship reliable, compliant software".into()),
        thread_context: Vec::new(),
        candidates: vec![
            candidate(
                "engineering",
                "Engineering",
                "software architecture and implementation",
            ),
            candidate("legal", "Legal", "contracts and compliance"),
        ],
        roster_version: 1,
        policy: RoutingPolicy {
            minimum_confidence: probability(600_000),
            high_impact_minimum_confidence: probability(800_000),
            collaboration_threshold: probability(600_000),
            contribution_threshold: probability(600_000),
            clarification_threshold: probability(700_000),
            high_impact_threshold: probability(700_000),
            round_width: 2,
            choice_option_limit: 8,
        },
    }
}

/// Build one available route candidate without host-only state.
fn candidate(id: &str, label: &str, description: &str) -> RouteCandidate {
    RouteCandidate {
        id: id.into(),
        label: label.into(),
        role: Some(label.into()),
        description: Some(description.into()),
        capabilities: vec![description.into()],
        learned_topics: Vec::new(),
        available: true,
    }
}

/// Convert a checked fixture value into fixed-point probability.
fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("fixture probability is bounded")
}

/// Run the proof on worker threads with enough stack for the embedded agent loop.
fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(WORKER_STACK_BYTES)
        .build()?;
    runtime.block_on(run())
}

/// Route once, run exactly the selected OpenHuman seat, and verify every call.
async fn run() -> anyhow::Result<()> {
    let transport = FixtureTransport::default();
    let router = JevRouter::new(transport);
    let request = request();
    let plan = route_message(Some(&router), None, &request, None, "engineering").await;
    let RoutingPlan::One { responder_id, .. } = &plan else {
        anyhow::bail!("fixture routing did not select one responder: {plan:?}");
    };
    if responder_id != "engineering" {
        anyhow::bail!("fixture routing selected unexpected responder {responder_id}");
    }

    let backend = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {"id": "openhuman-proof", "email": "local@openhuman.local"}
        })))
        .mount(&backend)
        .await;
    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_completion()))
        .mount(&provider)
        .await;

    let harness = Harness::builder()
        .config(offline_config())
        .workspace(Workspace::Ephemeral)
        .backend_url(backend.uri())
        .provider(
            Provider::openai_compatible(format!("{}/v1", provider.uri()), "local-test-key")
                .model("openhuman-proof-model"),
        )
        .access(Access::readonly())
        .build()
        .await?;
    let session_id = format!("tinyhivemind-openhuman:{responder_id}");
    let outcome = harness
        .turn(format!(
            "You are the {responder_id} seat. Answer without taking actions: {}",
            request.message
        ))
        .session(&session_id)
        .send()
        .await?;
    if outcome.reply != OPENHUMAN_REPLY || outcome.session_id != session_id {
        anyhow::bail!("embedded OpenHuman outcome did not match the proof contract");
    }
    if router.transport().calls.load(Ordering::SeqCst) != 1 {
        anyhow::bail!("ordinary desk routing did not make exactly one System One request");
    }
    let provider_requests = provider
        .received_requests()
        .await
        .ok_or_else(|| anyhow::anyhow!("mock provider did not retain requests"))?
        .len();
    if provider_requests != 1 {
        anyhow::bail!("embedded OpenHuman made {provider_requests} provider calls, expected one");
    }

    println!("route: {responder_id}");
    println!("system_one_calls: 1");
    println!("session: {}", outcome.session_id);
    println!("reply: {}", outcome.reply);
    Ok(())
}

/// Disable every optional local service the loopback proof does not need.
fn offline_config() -> RuntimeConfig {
    let mut config = RuntimeConfig::default();
    config.local_ai.runtime_enabled = false;
    config.runtime_python.enabled = false;
    config.memory_tree.spacy_enabled = false;
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;
    config.default_temperature = 0.0;
    config
}

/// Return the one OpenAI-compatible completion accepted by the proof.
fn chat_completion() -> serde_json::Value {
    json!({
        "id": "chatcmpl-openhuman-proof",
        "object": "chat.completion",
        "created": 1_700_000_000_u64,
        "model": "openhuman-proof-model",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": OPENHUMAN_REPLY},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 10, "completion_tokens": 2, "total_tokens": 12}
    })
}

#[cfg(test)]
mod tests {
    use super::{fixture_response, request};

    #[test]
    /// Keep the typed evaluator fixture aligned with the candidate snapshot.
    fn fixture_matches_the_candidate_snapshot() {
        let request = request();
        let response = fixture_response();
        assert_eq!(request.candidates.len(), 2);
        assert_eq!(response.model, "jev-fixture");
        assert!(response.answers.contains_key("primary_responder"));
    }

    #[test]
    /// Exercise routing, loopback provider IO, and the embedded Harness together.
    fn embedded_route_runs_to_completion() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_stack_size(super::WORKER_STACK_BYTES)
            .build()
            .expect("proof runtime builds");
        runtime
            .block_on(async { tokio::spawn(super::run()).await })
            .expect("proof task did not panic")
            .expect("proof completes");
    }
}
