//! The tools a seat's model request actually carries, captured offline.
//!
//! Real seats are built by the real factory on a real OpenHuman runtime; only
//! the provider (a loopback scripted model), the backend (the adapter's
//! loopback stub) and the memory engine (TinyMemory's in-process reference
//! engine) are local. Each seat runs one turn, and the request body's `tools`
//! is compared with the seat's package allow list.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use openhuman_embed::{Provider, Runtime, Workspace};
use serde_json::{Value, json};
use tinymemory_api::MemoryEngine;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use super::agents::{SeatFactory, TEMPLATE};
use super::artifacts::RunDir;
use super::budget::{Budget, Limits};
use super::floors::Assignment;
use super::memory::{SeatMemory, scope_binding};
use super::package;
use super::sandbox::PythonSandbox;
use super::tools::{Action, SeatScope, Services};
use super::turn::{Seat, TurnInput, run};

/// Answers every first call with `hive_complete`, records every body.
#[derive(Clone, Default)]
struct Scripted {
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl Respond for Scripted {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let answered = body["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .is_some_and(|message| message["role"] == "tool");
        self.bodies.lock().expect("capture lock").push(body);
        let message = if answered {
            json!({"role": "assistant", "content": "done"})
        } else {
            json!({"role": "assistant", "content": null, "tool_calls": [{
                "id": "call-1", "type": "function",
                "function": {"name": "hive_complete", "arguments": "{\"report\":\"offline report\"}"}
            }]})
        };
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "scripted", "object": "chat.completion", "created": 0, "model": "fixture",
            "choices": [{"index": 0, "message": message,
                "finish_reason": if answered { "stop" } else { "tool_calls" }}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }))
    }
}

fn tool_names(body: &Value) -> BTreeSet<String> {
    body["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| tool.pointer("/function/name").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn each_request_carries_exactly_the_seats_allowed_tools() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .expect("tokio runtime")
        .block_on(async { tokio::spawn(requests_carry_allowed_tools()).await })
        .expect("the request test completes");
}

async fn requests_carry_allowed_tools() {
    let documents = package::read_package(&super::test::package_dir()).expect("package reads");
    let (_, hive) = package::load(documents, "A test problem.").expect("package lowers");
    let provider = MockServer::start().await;
    let script = Scripted::default();
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(script.clone())
        .mount(&provider)
        .await;
    let backend = tinyhivemind_openhuman::offline::backend().await;
    let engine: Arc<dyn MemoryEngine> =
        Arc::new(tinymemory_api::conformance::reference::ReferenceEngine::new());
    let mut config = tinyhivemind_openhuman::offline::config();
    tinyhivemind_openhuman::HiveMemory::with_root(hive.memory.root.clone())
        .expect("root")
        .configure(&mut config);
    let runtime = Arc::new(
        Box::pin(
            Runtime::builder()
                .config(config)
                .workspace(Workspace::Ephemeral)
                .backend_url(backend.uri())
                .provider(
                    Provider::openai_compatible(format!("{}/v1", provider.uri()), "fixture")
                        .model("fixture"),
                )
                .memory_engine(engine.clone())
                .build(),
        )
        .await
        .expect("runtime builds"),
    );
    let factory = SeatFactory {
        runtime,
        charters: BTreeMap::new(),
        protocol: "protocol".into(),
        package_root: hive.memory.root.clone(),
        run: "offline".into(),
    };
    let dir = tempfile::tempdir().expect("run dir");
    let services = Arc::new(Services {
        search: None,
        sandbox: PythonSandbox::from_env(),
        budget: Arc::new(Budget::new(Limits::SMOKE)),
        run: Arc::new(RunDir::create(dir.path().join("run")).expect("run dir")),
        run_id: "offline".into(),
        literals: Vec::new(),
    });
    for id in ["theory-designer", "research-librarian", "verify-adversary"] {
        let seat = hive.seats.iter().find(|seat| seat.id == id).expect("seat");
        let tinyhivemind_openhuman::ManagementRequest::CreateAgent {
            template,
            config,
            memory,
            ..
        } = tinyhivemind_openhuman::language::management_request(seat)
        else {
            panic!("the adapter must produce CreateAgent");
        };
        assert_eq!(template, TEMPLATE);
        let binding = memory.map(|binding| *binding).expect("bound seat");
        let agent = tinyhivemind_openhuman::AgentFactory::create_with_memory(
            &factory,
            template,
            config,
            Some(binding.clone()),
        )
        .await
        .expect("factory builds the seat");
        let scoped = scope_binding(&binding, &hive.memory.root, "offline").expect("scoped");
        assert_eq!(
            agent.config().memory.root.as_deref(),
            Some(scoped.root.as_str())
        );
        let desk = package::desk_of(&hive, id).expect("desk");
        let belt = package::seat_tools(seat).expect("belt");
        let seat_memory = Arc::new(SeatMemory::new(engine.clone(), scoped, None, false));
        let scope = SeatScope {
            seat: id.into(),
            desk: desk.id.clone(),
            colleagues: desk.members.iter().filter(|m| *m != id).cloned().collect(),
            linked_desks: vec!["verify".into()],
            conversation: "offline".into(),
        };
        let seat_runtime = Arc::new(Seat::new(agent, belt.clone(), seat_memory, scope));
        let before = script.bodies.lock().expect("capture lock").len();
        let output = run(
            seat_runtime,
            TurnInput {
                problem: "A test problem.".into(),
                desk_line: format!("#{}", desk.id),
                rows: vec![],
                assignment: Assignment {
                    reason: "opening".into(),
                    note: "go".into(),
                },
                budget_line: "plenty".into(),
            },
            services.clone(),
        )
        .await;
        assert!(output.error.is_none(), "{id}: {:?}", output.error);
        assert!(
            matches!(output.action, Action::Complete { .. }),
            "{id} completed"
        );
        let bodies = script.bodies.lock().expect("capture lock");
        let first = &bodies[before];
        let expected: BTreeSet<String> = belt.iter().map(|tool| tool.name().to_owned()).collect();
        assert_eq!(tool_names(first), expected, "{id} request tools");
        let system = first["messages"][0]["content"].as_str().unwrap_or("");
        assert!(
            system.starts_with("You are a \""),
            "{id} system prompt is the seat's own"
        );
    }
}
