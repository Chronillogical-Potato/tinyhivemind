//! Routing wire, eligibility, fallback, bypass, and hive-invitation tests.

#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tinyhivemind::{Sequence, responder::Probability};

use super::*;
use crate::{ConversationKind, ConversationRef};

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("test probability is bounded")
}

fn request(kind: ConversationKind) -> RoutingRequest {
    RoutingRequest {
        message: "Assess the contract risk and implementation impact".into(),
        conversation: ConversationRef {
            id: "legal-engineering".into(),
            kind,
            thread_root: Some(Sequence(7)),
        },
        desk_purpose: Some("ship compliant software".into()),
        thread_context: vec!["operator: review before release".into()],
        candidates: vec![
            RouteCandidate {
                id: "eng".into(),
                label: "Engineer".into(),
                role: Some("implementation".into()),
                description: None,
                capabilities: vec!["rust".into()],
                learned_topics: vec!["routing".into()],
                available: true,
            },
            RouteCandidate {
                id: "legal".into(),
                label: "Counsel".into(),
                role: Some("compliance".into()),
                description: None,
                capabilities: vec!["contracts".into()],
                learned_topics: vec![],
                available: true,
            },
            RouteCandidate {
                id: "retired".into(),
                label: "Former operator".into(),
                role: None,
                description: None,
                capabilities: vec![],
                learned_topics: vec![],
                available: false,
            },
        ],
        roster_version: 9,
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

fn evaluation() -> RoutingEvaluation {
    RoutingEvaluation {
        primary_responder: "eng".into(),
        primary_probabilities: vec![
            CandidateProbability {
                candidate_id: "eng".into(),
                probability: probability(600_000),
            },
            CandidateProbability {
                candidate_id: "legal".into(),
                probability: probability(300_000),
            },
            CandidateProbability {
                candidate_id: "none".into(),
                probability: probability(100_000),
            },
        ],
        confidence: probability(750_000),
        needs_collaboration: probability(800_000),
        needs_clarification: probability(100_000),
        contributions: vec![
            ContributionProbability {
                candidate_id: "eng".into(),
                probability: probability(950_000),
            },
            ContributionProbability {
                candidate_id: "legal".into(),
                probability: probability(850_000),
            },
        ],
        high_impact: probability(100_000),
        model_identity: "jev-latest".into(),
        question_schema_version: 1,
        roster_version: 9,
        disposition: EvaluationDisposition::Unchecked,
    }
}

#[derive(Debug)]
struct FakeRouter {
    calls: Arc<AtomicUsize>,
    evaluation: RoutingEvaluation,
}

impl Router for FakeRouter {
    fn evaluate<'a>(&'a self, _request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let evaluation = self.evaluation.clone();
        Box::pin(async move { Ok(evaluation) })
    }
}

#[tokio::test]
async fn explicit_mentions_and_direct_conversations_bypass_semantic_routing() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = FakeRouter {
        calls: Arc::clone(&calls),
        evaluation: evaluation(),
    };
    let mentioned = route_message(
        Some(&router),
        None,
        &request(ConversationKind::Desk),
        Some("legal"),
        "eng",
    )
    .await;
    assert!(matches!(
        mentioned,
        RoutingPlan::Fallback {
            reason: RoutingFallback::ExplicitMention,
            ..
        }
    ));
    let direct = route_message(
        Some(&router),
        None,
        &request(ConversationKind::Direct),
        None,
        "legal",
    )
    .await;
    assert!(matches!(
        direct,
        RoutingPlan::Fallback {
            reason: RoutingFallback::DirectConversation,
            ..
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn contribution_nouls_open_a_bounded_hive_in_desk_order_after_probability() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = FakeRouter {
        calls: Arc::clone(&calls),
        evaluation: evaluation(),
    };
    let plan = route_message(
        Some(&router),
        None,
        &request(ConversationKind::Desk),
        None,
        "eng",
    )
    .await;
    let RoutingPlan::Hive {
        primary_id,
        invited_ids,
        evaluation,
    } = plan
    else {
        panic!("expected a hive plan");
    };
    assert_eq!(primary_id, "eng");
    assert_eq!(invited_ids, ["legal"]);
    assert_eq!(evaluation.disposition, EvaluationDisposition::Accepted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unavailable_candidate_in_a_distribution_is_rejected() {
    let mut invalid = evaluation();
    invalid.primary_probabilities.pop();
    invalid.primary_probabilities.push(CandidateProbability {
        candidate_id: "retired".into(),
        probability: probability(100_000),
    });
    let router = FakeRouter {
        calls: Arc::new(AtomicUsize::new(0)),
        evaluation: invalid,
    };
    let plan = route_message(
        Some(&router),
        None,
        &request(ConversationKind::Desk),
        None,
        "eng",
    )
    .await;
    assert_eq!(
        plan,
        RoutingPlan::Fallback {
            responder_id: "eng".into(),
            reason: RoutingFallback::RejectedOutput
        }
    );
}

#[tokio::test]
async fn uncertainty_receives_exactly_one_reasoning_escalation() {
    let mut uncertain = evaluation();
    uncertain.confidence = probability(500_000);
    let primary_calls = Arc::new(AtomicUsize::new(0));
    let reasoning_calls = Arc::new(AtomicUsize::new(0));
    let primary = FakeRouter {
        calls: Arc::clone(&primary_calls),
        evaluation: uncertain,
    };
    let reasoning = FakeRouter {
        calls: Arc::clone(&reasoning_calls),
        evaluation: evaluation(),
    };
    let plan = route_message(
        Some(&primary),
        Some(&reasoning),
        &request(ConversationKind::Desk),
        None,
        "eng",
    )
    .await;
    assert!(matches!(plan, RoutingPlan::Hive { .. }));
    assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
    assert_eq!(reasoning_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn conflicting_collaboration_judgments_escalate_then_fall_back() {
    let mut conflicting = evaluation();
    for contribution in &mut conflicting.contributions {
        contribution.probability = probability(100_000);
    }
    let primary_calls = Arc::new(AtomicUsize::new(0));
    let reasoning_calls = Arc::new(AtomicUsize::new(0));
    let primary = FakeRouter {
        calls: Arc::clone(&primary_calls),
        evaluation: conflicting.clone(),
    };
    let reasoning = FakeRouter {
        calls: Arc::clone(&reasoning_calls),
        evaluation: conflicting,
    };
    let plan = route_message(
        Some(&primary),
        Some(&reasoning),
        &request(ConversationKind::Desk),
        None,
        "eng",
    )
    .await;
    assert_eq!(
        plan,
        RoutingPlan::Fallback {
            responder_id: "eng".into(),
            reason: RoutingFallback::EscalationFailed,
        }
    );
    assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
    assert_eq!(reasoning_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn malformed_candidate_snapshots_are_rejected_before_a_provider_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = FakeRouter {
        calls: Arc::clone(&calls),
        evaluation: evaluation(),
    };
    let mut malformed = request(ConversationKind::Desk);
    malformed.candidates[1].id = malformed.candidates[0].id.clone();
    let plan = route_message(Some(&router), None, &malformed, None, "eng").await;
    assert!(matches!(
        plan,
        RoutingPlan::Fallback {
            reason: RoutingFallback::RejectedOutput,
            ..
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn conversation_and_route_wires_are_explicit() {
    let value = serde_json::to_value(ConversationRef {
        id: "dm-42".into(),
        kind: ConversationKind::Direct,
        thread_root: None,
    })
    .expect("conversation serializes");
    assert_eq!(
        value,
        serde_json::json!({"id":"dm-42","kind":"direct","thread_root":null})
    );
    let route = serde_json::to_value(crate::MessageRoute::DirectAgent {
        agent_id: "legal".into(),
    })
    .expect("route serializes");
    assert_eq!(
        route,
        serde_json::json!({"kind":"direct_agent","agent_id":"legal"})
    );
}
