//! Behavior tests for binding accepted routes to existing agent instances.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use crate::{EvaluationDisposition, RoutingEvaluation, RoutingFallback, RoutingPlan};
use tinyhivemind::responder::Probability;

#[derive(Debug, Eq, PartialEq)]
struct Agent(&'static str);

fn agents() -> AgentRegistry<Agent> {
    AgentRegistry::new([
        ("engineering", Agent("same engineering instance")),
        ("legal", Agent("same legal instance")),
    ])
    .expect("registry is valid")
}

fn fallback(id: &str) -> RoutingPlan {
    RoutingPlan::Fallback {
        responder_id: id.to_string(),
        reason: RoutingFallback::DirectConversation,
    }
}

fn evaluation() -> RoutingEvaluation {
    RoutingEvaluation {
        primary_responder: "engineering".into(),
        primary_probabilities: Vec::new(),
        confidence: Probability::new(900_000).expect("valid probability"),
        needs_collaboration: Probability::ZERO,
        needs_clarification: Probability::ZERO,
        contributions: Vec::new(),
        high_impact: Probability::ZERO,
        model_identity: "fixture".into(),
        question_schema_version: 1,
        roster_version: 1,
        disposition: EvaluationDisposition::Accepted,
    }
}

#[test]
fn repeated_routes_return_the_same_instantiated_agent() {
    let agents = agents();
    let first = agents.resolve(&fallback("engineering")).expect("resolves");
    let second = agents.resolve(&fallback("engineering")).expect("resolves");
    let (RoutedAgents::One(first), RoutedAgents::One(second)) = (first, second) else {
        panic!("fallbacks resolve one agent")
    };

    assert!(std::ptr::eq(first.agent, second.agent));
    assert_eq!(first.agent, &Agent("same engineering instance"));
}

#[test]
fn routing_never_constructs_a_missing_agent() {
    assert_eq!(
        agents().resolve(&fallback("finance")).unwrap_err(),
        AgentRegistryError::MissingAgent("finance".to_string())
    );
}

#[test]
fn blank_and_duplicate_registry_ids_fail_closed() {
    assert_eq!(
        AgentRegistry::new([(" ", Agent("blank"))]).unwrap_err(),
        AgentRegistryError::BlankId
    );
    assert_eq!(
        AgentRegistry::new([
            ("engineering", Agent("first")),
            ("engineering", Agent("second")),
        ])
        .unwrap_err(),
        AgentRegistryError::DuplicateId("engineering".to_string())
    );
}

#[test]
fn a_hive_preserves_primary_then_invitation_order() {
    let agents = agents();
    let plan = RoutingPlan::Hive {
        primary_id: "engineering".into(),
        invited_ids: vec!["legal".into()],
        evaluation: evaluation(),
    };
    let RoutedAgents::Hive { primary, invited } = agents.resolve(&plan).expect("hive resolves")
    else {
        panic!("hive plan resolves a hive")
    };

    assert_eq!(primary.id, "engineering");
    assert_eq!(invited.len(), 1);
    assert_eq!(invited[0].id, "legal");
}

#[test]
fn a_clarification_authorizes_no_agent_turn() {
    let plan = RoutingPlan::Clarify {
        evaluation: evaluation(),
    };

    assert!(matches!(
        agents().resolve(&plan).expect("clarification resolves"),
        RoutedAgents::Clarify
    ));
}

#[test]
fn a_hive_cannot_repeat_an_instantiated_agent() {
    let plan = RoutingPlan::Hive {
        primary_id: "engineering".into(),
        invited_ids: vec!["engineering".into()],
        evaluation: evaluation(),
    };

    assert_eq!(
        agents().resolve(&plan).unwrap_err(),
        AgentRegistryError::DuplicateRoutedAgent("engineering".into())
    );
}
