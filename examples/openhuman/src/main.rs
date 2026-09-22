//! Standalone proof: route requests onto instantiated OpenHuman agents.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use openhuman_core::agent::registry::types::{
    AgentRegistryEntry, AgentRegistrySource, AgentSubagentPolicy,
};
use openhuman_embed::{Access, Agent, AgentSpec, Provider, Runtime, RuntimeConfig, Workspace};
use serde_json::json;
use tinyhivemind::{
    desk::{Desk, ResponderMode},
    responder::Probability,
};
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, MessageRoute, RouteCandidate, RoutingPolicy, RoutingRequest,
};
use tinyhivemind_openhuman::EmbedSeat;
use tinyhivemind_driver::{AgentBinding, HiveGraph, BoundHive};
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
        source: tinyhivemind_embed::RoutingSource::DeskMessage,
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

/// Build one OpenHuman runtime, instantiate its agents, and route onto them.
async fn run() -> anyhow::Result<()> {
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

    let runtime = Runtime::builder()
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

    // OpenHuman owns the runtime and agent lifecycle. TinyHiveMind receives
    // the already-instantiated handles and never serializes or reconstructs
    // them between turns.
    let hive = BoundHive::new(
        HiveGraph::new(
            Desk {
                id: "launch".into(),
                name: "Launch".into(),
                description: Some("ship reliable, compliant software".into()),
                members: vec!["engineering".into(), "legal".into()],
                responder_mode: ResponderMode::Auto,
            },
            request().candidates,
        ),
        vec![
            AgentBinding::new(
                "engineering",
                EmbedSeat(runtime.agent(
                    AgentSpec::new("engineering")
                        .system_prompt("You are the engineering specialist.")
                        .config(|config| {
                            config.agent_registry.entries.push(registry_entry(
                                "engineering",
                                "You are the engineering specialist.",
                            ));
                        }),
                )?),
            ),
            AgentBinding::new(
                "legal",
                EmbedSeat(runtime.agent(
                    AgentSpec::new("legal")
                        .system_prompt("You are the legal specialist.")
                        .config(|config| {
                            config
                                .agent_registry
                                .entries
                                .push(registry_entry("legal", "You are the legal specialist."));
                        }),
                )?),
            ),
        ],
    )?;
    if runtime.agent_ids() != ["engineering".to_string(), "legal".to_string()] {
        anyhow::bail!("OpenHuman runtime did not retain both instantiated agents");
    }

    let transport = FixtureTransport::default();
    let router = JevRouter::new(transport);
    let fixture_request = request();
    let desk_request = hive.desk_request(
        fixture_request.message,
        fixture_request.thread_context,
        fixture_request.conversation.thread_root,
        fixture_request.roster_version,
        fixture_request.policy,
    );
    let desk_plan = hive
        .route_desk(Some(&router), None, &desk_request, None, "engineering")
        .await?;
    let resolved = hive.resolve_plan(&desk_plan)?;
    let [engineering] = resolved.as_slice() else {
        anyhow::bail!("fixture routing did not select one responder: {desk_plan:?}");
    };
    let session_id = openhuman_session(&engineering.agent.0);
    let first = run_turn(&engineering.agent.0, &session_id, &desk_request.message).await?;

    // The same instantiated OpenHuman agent crosses from a desk into a DM.
    // The deterministic DM route bypasses Jev, and OpenHuman receives the same
    // session id, so its own transcript/compaction layer carries continuity.
    let dm_message = "Follow up privately with the implementation risk.";
    let dm_route = hive.resolve_dm("legal", &["engineering".into()], 2)?;
    let MessageRoute::DeskAside { recipient_ids, .. } = dm_route else {
        anyhow::bail!("private hive route escaped the desk");
    };
    let [recipient_id] = recipient_ids.as_slice() else {
        anyhow::bail!("private hive route did not select exactly one responder");
    };
    let engineering_again = hive
        .binding(recipient_id)
        .ok_or_else(|| anyhow::anyhow!("private route selected an unbound agent"))?;
    if engineering.runtime_agent_id() != engineering_again.runtime_agent_id() {
        anyhow::bail!("surface change replaced the instantiated OpenHuman agent");
    }
    let second = run_turn(&engineering_again.agent.0, &session_id, dm_message).await?;
    if first.reply != OPENHUMAN_REPLY
        || second.reply != OPENHUMAN_REPLY
        || first.session_id != session_id
        || second.session_id != session_id
    {
        anyhow::bail!("OpenHuman did not retain one session across both surfaces");
    }
    if router.transport().calls.load(Ordering::SeqCst) != 1 {
        anyhow::bail!("the desk plus DM did not make exactly one System One request");
    }
    let provider_requests = provider
        .received_requests()
        .await
        .ok_or_else(|| anyhow::anyhow!("mock provider did not retain requests"))?;
    if provider_requests.len() != 2 {
        anyhow::bail!(
            "embedded OpenHuman made {} provider calls, expected two",
            provider_requests.len()
        );
    }
    let first_body: serde_json::Value = serde_json::from_slice(&provider_requests[0].body)?;
    let second_body: serde_json::Value = serde_json::from_slice(&provider_requests[1].body)?;
    let first_messages = first_body["messages"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("first OpenHuman request has no message history"))?;
    let second_messages = second_body["messages"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("second OpenHuman request has no message history"))?;
    if !second_messages.starts_with(first_messages) {
        anyhow::bail!("OpenHuman changed the prior message prefix between agent turns");
    }
    let retained_first_reply = second_messages
        .iter()
        .any(|message| message["role"] == "assistant" && message["content"] == OPENHUMAN_REPLY);
    if !retained_first_reply {
        anyhow::bail!("OpenHuman did not carry the first reply into the second turn");
    }

    println!("agents: engineering,legal");
    println!("desk_route: {}", engineering.hive_agent_id);
    println!("direct_route: {}", engineering_again.hive_agent_id);
    println!("system_one_calls: 1");
    println!("openhuman_session: {session_id}");
    println!("turns_on_same_session: 2");
    println!(
        "cacheable_prefix_messages: {}/{}",
        first_messages.len(),
        first_messages.len()
    );
    println!("reply: {}", second.reply);
    Ok(())
}

fn registry_entry(id: &str, system_prompt: &str) -> AgentRegistryEntry {
    AgentRegistryEntry {
        id: id.into(),
        name: id.into(),
        description: format!("Embedded {id} proof agent"),
        source: AgentRegistrySource::Custom,
        enabled: true,
        model: None,
        system_prompt: Some(system_prompt.into()),
        tool_allowlist: Vec::new(),
        tool_denylist: Vec::new(),
        subagents: AgentSubagentPolicy::default(),
        tags: Vec::new(),
        metadata: serde_json::Value::Null,
    }
}

/// OpenHuman's per-agent transcript key; TinyHiveMind stores no session state.
fn openhuman_session(agent: &Agent) -> String {
    format!("tinyhivemind-embed:{}", agent.id())
}

/// Send one turn through the already-instantiated OpenHuman agent.
async fn run_turn(
    agent: &Agent,
    session_id: &str,
    message: &str,
) -> Result<openhuman_embed::TurnOutcome, openhuman_embed::AgentError> {
    agent
        .turn(format!(
            "You are the {} seat. Answer without taking actions: {message}",
            agent.id()
        ))
        .session(session_id)
        .send()
        .await
        .map_err(Into::into)
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
    /// Exercise routing, loopback provider IO, and the embedded runtime together.
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
