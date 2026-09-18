//! System-scale routing invariants over mixed conversation surfaces.

#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use tinyhivemind::responder::Probability;
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, ConversationKind, ConversationRef,
    EvaluationDisposition, RouteCandidate, Router, RouterFuture, RoutingEvaluation, RoutingPlan,
    RoutingPolicy, RoutingRequest, route_message,
};

#[derive(Clone, Debug)]
struct SnapshotRouter {
    calls: Arc<AtomicUsize>,
}

impl Router for SnapshotRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let evaluation = evaluation(request);
        Box::pin(async move { Ok(evaluation) })
    }
}

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("test probability is bounded")
}

fn evaluation(request: &RoutingRequest) -> RoutingEvaluation {
    let eligible: Vec<_> = request
        .candidates
        .iter()
        .filter(|candidate| candidate.available)
        .collect();
    let primary = eligible[0].id.clone();
    let tail_count = u32::try_from(eligible.len() - 3).expect("several candidates");
    let tail_each = 10_000 / tail_count;
    let mut tail_assigned = 0_u32;
    let mut primary_probabilities = vec![CandidateProbability {
        candidate_id: primary.clone(),
        probability: probability(450_000),
    }];
    for (index, candidate) in eligible.iter().skip(1).enumerate() {
        let parts = match index {
            0 | 1 => 220_000,
            _ if index + 2 == eligible.len() => 10_000 - tail_assigned,
            _ => {
                tail_assigned += tail_each;
                tail_each
            }
        };
        primary_probabilities.push(CandidateProbability {
            candidate_id: candidate.id.clone(),
            probability: probability(parts),
        });
    }
    primary_probabilities.push(CandidateProbability {
        candidate_id: "none".into(),
        probability: probability(100_000),
    });
    RoutingEvaluation {
        primary_responder: primary,
        primary_probabilities,
        confidence: probability(850_000),
        needs_collaboration: probability(800_000),
        needs_clarification: probability(20_000),
        contributions: eligible
            .iter()
            .enumerate()
            .map(|(index, candidate)| ContributionProbability {
                candidate_id: candidate.id.clone(),
                probability: probability(
                    900_000 - u32::try_from(index).expect("small index") * 20_000,
                ),
            })
            .collect(),
        high_impact: probability(100_000),
        model_identity: "scale-simulator".into(),
        question_schema_version: 1,
        roster_version: request.roster_version,
        disposition: EvaluationDisposition::Unchecked,
    }
}

fn request(desk: usize, kind: ConversationKind) -> RoutingRequest {
    RoutingRequest {
        message: format!("coordinate request for desk {desk}"),
        conversation: ConversationRef {
            id: format!("desk-{desk}"),
            kind,
            thread_root: None,
        },
        desk_purpose: Some("scale simulation".into()),
        thread_context: vec![],
        candidates: (0..10)
            .map(|seat| RouteCandidate {
                id: format!("agent-{desk}-{seat}"),
                label: format!("Agent {desk}-{seat}"),
                role: Some(format!("specialty-{seat}")),
                description: None,
                capabilities: vec![format!("capability-{seat}")],
                learned_topics: vec![],
                available: true,
            })
            .collect(),
        roster_version: u64::try_from(desk).expect("desk index fits") + 1,
        policy: RoutingPolicy {
            minimum_confidence: probability(600_000),
            high_impact_minimum_confidence: probability(800_000),
            clarification_threshold: probability(700_000),
            high_impact_threshold: probability(700_000),
            round_width: 4,
            choice_option_limit: 32,
        },
    }
}

#[tokio::test]
async fn one_thousand_agents_across_one_hundred_desks_remain_bounded() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = SnapshotRouter {
        calls: Arc::clone(&calls),
    };
    for desk in 0..100 {
        let plan = route_message(
            Some(&router),
            None,
            &request(desk, ConversationKind::Desk),
            None,
            &format!("agent-{desk}-0"),
        )
        .await;
        let RoutingPlan::Hive { invited_ids, .. } = plan else {
            panic!("desk request should open a hive");
        };
        assert_eq!(invited_ids.len(), 2);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 100);
}

#[tokio::test]
async fn mixed_non_desk_traffic_never_invokes_the_router() {
    let calls = Arc::new(AtomicUsize::new(0));
    let router = SnapshotRouter {
        calls: Arc::clone(&calls),
    };
    for desk in 0..100 {
        for kind in [
            ConversationKind::Direct,
            ConversationKind::General,
            ConversationKind::Workflow,
        ] {
            let plan = route_message(
                Some(&router),
                None,
                &request(desk, kind),
                None,
                &format!("agent-{desk}-0"),
            )
            .await;
            assert!(matches!(plan, RoutingPlan::Fallback { .. }));
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
