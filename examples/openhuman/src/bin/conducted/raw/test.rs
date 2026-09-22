//! The raw runner's pure pieces: the belt and the gate.

use std::sync::Arc;

use openhuman_core::agent::tool_policy::{
    ToolCallContext, ToolPolicy, ToolPolicyDecision, ToolPolicyRequest,
};
use serde_json::json;
use tinyhivemind_mcp::{Dispatch, EpisodeTools};
use tinytools::PermissionLevel;

use super::policy::EpisodeGate;
use super::tools;

#[test]
fn the_belt_is_the_served_vocabulary_and_only_read_is_read_only() {
    let tools = Arc::new(EpisodeTools::new(["lead", "solver"]));
    let belt = tools::belt("lead", &tools);
    let names: Vec<&str> = belt.iter().map(|tool| tool.name()).collect();
    let served: Vec<&str> = tinyhivemind_mcp::served_specs()
        .map(|spec| spec.name)
        .collect();
    assert_eq!(names, served);
    for tool in &belt {
        assert_eq!(
            tool.permission_level() == PermissionLevel::ReadOnly,
            tool.name() == "read",
            "{}",
            tool.name()
        );
        assert!(
            tool.description().len() > 20,
            "{} carries the spec's words",
            tool.name()
        );
        let required = tool.parameters_schema()["required"].clone();
        assert!(
            required
                .as_array()
                .is_some_and(|r| r.iter().any(|v| v == "chat")),
            "{} names the chat it is in",
            tool.name()
        );
    }
}

#[tokio::test]
async fn a_native_call_is_recorded_through_the_shared_record() {
    let tools = Arc::new(EpisodeTools::new(["lead", "solver"]));
    tools.register(
        "lead",
        Dispatch {
            chat: "engineering".into(),
            parent: None,
        },
    );
    let belt = tools::belt("lead", &tools);
    let complete = belt
        .iter()
        .find(|tool| tool.name() == "complete_episode")
        .expect("served");
    let accepted = complete
        .execute(json!({"message": "done", "chat": "engineering", "parent": null}))
        .await
        .expect("executes");
    assert!(!accepted.is_error);
    let refused = complete
        .execute(json!({"message": "done", "chat": "elsewhere"}))
        .await
        .expect("executes");
    assert!(refused.is_error, "the wire's refusal is the tool's error");
    assert_eq!(
        tools.drain("lead").len(),
        1,
        "only the accepted call was recorded"
    );
}

fn request(tool: &str) -> ToolPolicyRequest {
    #[allow(deprecated)]
    ToolPolicyRequest {
        tool_name: tool.to_owned(),
        arguments: json!({}),
        context: ToolCallContext::session("session", "internal", "lead", "call-1", 1),
        generated_tool: None,
        session_id: String::new(),
        channel: String::new(),
        agent_definition_id: String::new(),
    }
}

#[tokio::test]
async fn the_gate_admits_the_belt_and_denies_the_rest() {
    let gate = EpisodeGate::new(vec!["complete_episode".into(), "read".into()]);
    assert!(matches!(
        gate.check(&request("complete_episode")).await,
        ToolPolicyDecision::Allow
    ));
    assert!(matches!(
        gate.check(&request("shell")).await,
        ToolPolicyDecision::Deny { .. }
    ));
}
