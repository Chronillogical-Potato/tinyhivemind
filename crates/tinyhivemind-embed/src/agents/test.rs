//! Behavior tests for binding accepted routes to existing agent instances.

use super::*;
use crate::{EvaluationDisposition, RoutingEvaluation, RoutingFallback, RoutingPlan};
use tinyhivemind::responder::Probability;

#[derive(Debug, Eq, PartialEq)]
struct Agent(&'static str);

struct NotCloneOrCopy;

fn assert_clone<T: Clone>() {}

fn assert_copy<T: Copy>() {}

fn agents() -> Result<AgentRegistry<Agent>, AgentRegistryError> {
    AgentRegistry::new([
        ("engineering", Agent("same engineering instance")),
        ("legal", Agent("same legal instance")),
    ])
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
        confidence: Probability::ZERO,
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
fn repeated_routes_return_the_same_instantiated_agent() -> Result<(), Box<dyn std::error::Error>> {
    let agents = agents()?;
    let first = agents.resolve(&fallback("engineering"))?;
    let second = agents.resolve(&fallback("engineering"))?;
    let (RoutedAgents::One(first), RoutedAgents::One(second)) = (first, second) else {
        return Err("fallbacks did not resolve one agent".into());
    };

    assert!(std::ptr::eq(first.agent, second.agent));
    assert_eq!(first.agent, &Agent("same engineering instance"));
    Ok(())
}

#[test]
fn routing_never_constructs_a_missing_agent() {
    assert_eq!(
        agents()
            .and_then(|agents| agents.resolve(&fallback("finance")).map(|_| ()))
            .err(),
        Some(AgentRegistryError::MissingAgent("finance".to_string()))
    );
}

#[test]
fn blank_and_duplicate_registry_ids_fail_closed() {
    assert_eq!(
        AgentRegistry::new([(" ", Agent("blank"))]).err(),
        Some(AgentRegistryError::BlankId)
    );
    assert_eq!(
        AgentRegistry::new([
            ("engineering", Agent("first")),
            ("engineering", Agent("second")),
        ])
        .err(),
        Some(AgentRegistryError::DuplicateId("engineering".to_string()))
    );
}

#[test]
fn a_hive_preserves_primary_then_invitation_order() -> Result<(), Box<dyn std::error::Error>> {
    let agents = agents()?;
    let plan = RoutingPlan::Hive {
        primary_id: "engineering".into(),
        invited_ids: vec!["legal".into()],
        evaluation: evaluation(),
    };
    let RoutedAgents::Hive { primary, invited } = agents.resolve(&plan)? else {
        return Err("hive plan did not resolve a hive".into());
    };

    assert_eq!(primary.id, "engineering");
    assert_eq!(invited.len(), 1);
    assert_eq!(invited[0].id, "legal");
    Ok(())
}

#[test]
fn a_clarification_authorizes_no_agent_turn() -> Result<(), AgentRegistryError> {
    let plan = RoutingPlan::Clarify {
        evaluation: evaluation(),
    };

    assert!(matches!(agents()?.resolve(&plan)?, RoutedAgents::Clarify));
    Ok(())
}

#[test]
fn a_hive_cannot_repeat_an_instantiated_agent() {
    let plan = RoutingPlan::Hive {
        primary_id: "engineering".into(),
        invited_ids: vec!["engineering".into()],
        evaluation: evaluation(),
    };

    assert_eq!(
        agents()
            .and_then(|agents| agents.resolve(&plan).map(|_| ()))
            .err(),
        Some(AgentRegistryError::DuplicateRoutedAgent(
            "engineering".into()
        ))
    );
}

#[test]
fn borrowed_route_views_do_not_require_traits_from_the_agent() {
    assert_clone::<RoutedAgent<'static, NotCloneOrCopy>>();
    assert_copy::<RoutedAgent<'static, NotCloneOrCopy>>();
    assert_clone::<RoutedAgents<'static, NotCloneOrCopy>>();
}

#[test]
fn registry_lookup_and_borrowed_view_clones_preserve_the_same_instances()
-> Result<(), Box<dyn std::error::Error>> {
    let registry = agents()?;
    assert_eq!(
        registry.get("engineering"),
        Some(&Agent("same engineering instance"))
    );
    assert_eq!(registry.get("missing"), None);

    let one = registry.resolve(&fallback("engineering"))?;
    let RoutedAgents::One(original) = &one else {
        return Err("fallback did not resolve one agent".into());
    };
    let copied = *original;
    let cloned = one.clone();
    let RoutedAgents::One(cloned) = cloned else {
        return Err("cloned one-agent view changed shape".into());
    };
    assert!(std::ptr::eq(copied.agent, cloned.agent));

    let hive = registry.resolve(&RoutingPlan::Hive {
        primary_id: "engineering".into(),
        invited_ids: vec!["legal".into()],
        evaluation: evaluation(),
    })?;
    let RoutedAgents::Hive { primary, invited } = hive.clone() else {
        return Err("cloned hive view changed shape".into());
    };
    let engineering = registry.get("engineering").ok_or("missing engineering")?;
    let legal = registry.get("legal").ok_or("missing legal")?;
    assert!(std::ptr::eq(primary.agent, engineering));
    assert!(std::ptr::eq(invited[0].agent, legal));

    assert!(matches!(
        RoutedAgents::<Agent>::Clarify.clone(),
        RoutedAgents::Clarify
    ));
    Ok(())
}
