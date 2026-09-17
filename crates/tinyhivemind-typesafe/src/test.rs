//! Tests for exact Jev requests, conversion, hierarchy, and retry policy.

#![allow(
    clippy::cast_precision_loss,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used
)]

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};

use serde_json::json;
use tinyhivemind::{Sequence, responder::Probability};
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, RouteCandidate, Router, RoutingPolicy, RoutingRequest,
};

use super::*;

#[derive(Debug)]
struct FakeTransport {
    responses: Mutex<VecDeque<SystemOneResponse>>,
    requests: Mutex<Vec<SystemOneRequest>>,
}

impl FakeTransport {
    fn new(responses: impl IntoIterator<Item = SystemOneResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl SystemOneTransport for FakeTransport {
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a> {
        self.requests.lock().unwrap().push(request.clone());
        let response = self.responses.lock().unwrap().pop_front();
        Box::pin(async move {
            response.ok_or_else(|| TransportError {
                status: None,
                message: "no fake response".into(),
            })
        })
    }
}

fn request(candidate_count: usize, option_limit: usize) -> RoutingRequest {
    RoutingRequest {
        message: "Review the rollout and its legal exposure".into(),
        conversation: ConversationRef {
            id: "launch".into(),
            kind: ConversationKind::Desk,
            thread_root: Some(Sequence(4)),
        },
        desk_purpose: Some("coordinate launch".into()),
        thread_context: vec!["operator: launch is Friday".into()],
        candidates: (0..candidate_count)
            .map(|index| RouteCandidate {
                id: format!("agent-{index}"),
                label: format!("Agent {index}"),
                role: Some(if index == 0 { "engineering" } else { "legal" }.into()),
                description: None,
                capabilities: vec![format!("capability-{index}")],
                learned_topics: vec![],
                available: true,
            })
            .collect(),
        roster_version: 12,
        policy: RoutingPolicy {
            minimum_confidence: Probability::ZERO,
            high_impact_minimum_confidence: Probability::ZERO,
            collaboration_threshold: Probability::ONE,
            contribution_threshold: Probability::ONE,
            clarification_threshold: Probability::ONE,
            high_impact_threshold: Probability::ONE,
            round_width: 2,
            choice_option_limit: option_limit,
        },
    }
}

fn response(candidate_ids: &[&str], choice: &str, include_primary: bool) -> SystemOneResponse {
    let equal = 1.0 / candidate_ids.len() as f64;
    let mut answers = BTreeMap::from([
        (
            "needs_collaboration".into(),
            SystemOneAnswer::Noul(NoulAnswer { noul: 0.2 }),
        ),
        (
            "needs_clarification".into(),
            SystemOneAnswer::Noul(NoulAnswer { noul: 0.1 }),
        ),
        (
            "high_impact".into(),
            SystemOneAnswer::Noul(NoulAnswer { noul: 0.3 }),
        ),
    ]);
    for candidate in candidate_ids.iter().filter(|id| **id != "none") {
        answers.insert(
            format!("contributes_{candidate}"),
            SystemOneAnswer::Noul(NoulAnswer { noul: 0.7 }),
        );
    }
    if include_primary {
        answers.insert(
            "primary_responder".into(),
            SystemOneAnswer::Choice(ChoiceAnswer {
                choice: choice.into(),
                probabilities: candidate_ids
                    .iter()
                    .map(|id| ((*id).into(), equal))
                    .collect(),
                confidence: 0.8,
            }),
        );
    }
    SystemOneResponse {
        model: "jev-1.13".into(),
        answers,
        usage: TokenUsage {
            input_tokens: 100,
            output_tokens: 20,
        },
    }
}

#[tokio::test]
async fn ordinary_desk_is_one_batched_request_with_choice_and_all_nouls() {
    let transport =
        FakeTransport::new([response(&["agent-0", "agent-1", "none"], "agent-0", true)]);
    let router = JevRouter::new(transport);
    let evaluation = router
        .evaluate(&request(2, 8))
        .await
        .expect("evaluation converts");
    assert_eq!(evaluation.primary_responder, "agent-0");
    assert_eq!(evaluation.model_identity, "jev-1.13");
    assert_eq!(evaluation.question_schema_version, 1);
    let requests = router.transport().requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].questions.len(), 6);
    assert!(matches!(
        requests[0].questions["primary_responder"],
        Question::Choice { .. }
    ));
    assert_eq!(requests[0].state["roster_version"], 12);
}

#[tokio::test]
async fn large_desk_uses_bounded_screening_then_shortlist_choice() {
    let screened = response(
        &["agent-0", "agent-1", "agent-2", "agent-3"],
        "agent-0",
        false,
    );
    let final_response = response(&["agent-0", "agent-1", "none"], "agent-0", true);
    let transport = FakeTransport::new([screened, final_response]);
    let router = JevRouter::new(transport);
    let evaluation = router
        .evaluate(&request(4, 3))
        .await
        .expect("hierarchy converts");
    assert_eq!(evaluation.primary_probabilities.len(), 5);
    assert_eq!(router.transport().requests.lock().unwrap().len(), 2);
    let requests = router.transport().requests.lock().unwrap();
    assert!(!requests[0].questions.contains_key("primary_responder"));
    let Question::Choice { criteria, .. } = &requests[1].questions["primary_responder"] else {
        panic!("final question is Choice");
    };
    assert_eq!(criteria.len(), 3);
}

#[test]
fn wire_shapes_match_the_system_one_api() {
    let request = SystemOneRequest {
        state: json!({"message":"hello"}),
        model: "jev-latest".into(),
        questions: BTreeMap::from([(
            "urgent".into(),
            Question::Noul {
                instructions: json!("Is this urgent?"),
                criteria: None,
            },
        )]),
    };
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({
            "state":{"message":"hello"}, "model":"jev-latest",
            "questions":{"urgent":{"type":"noul","instructions":"Is this urgent?"}}
        })
    );
    let parsed: SystemOneResponse = serde_json::from_value(json!({
        "model":"jev-latest", "answers":{"urgent":{"type":"noul","noul":0.92}},
        "usage":{"input_tokens":312,"output_tokens":48}
    }))
    .unwrap();
    assert_eq!(
        parsed.answers["urgent"],
        SystemOneAnswer::Noul(NoulAnswer { noul: 0.92 })
    );
}

#[test]
fn only_rate_limit_and_overload_are_retryable() {
    assert_eq!(classify_retry(429), RetryClass::Retryable);
    assert_eq!(classify_retry(529), RetryClass::Retryable);
    assert_eq!(classify_retry(401), RetryClass::Permanent);
    assert_eq!(classify_retry(422), RetryClass::Permanent);
}
