//! Jev-first routing with deterministic bypass, acceptance, escalation, and fallback.

#[cfg(test)]
mod test;

mod types;

pub use types::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterError, RouterFuture, RoutingEvaluation, RoutingFallback, RoutingPlan, RoutingPolicy,
    RoutingRequest, RoutingSource,
};

use std::collections::BTreeSet;

use tinyhivemind::responder::PROBABILITY_SCALE;

use crate::ConversationKind;

/// Choice probability above which an additional eligible desk agent receives
/// the message in the same bounded opening round.
///
/// The comparison is strict: exactly 20% remains single-responder routing.
pub const CONCURRENT_CHOICE_THRESHOLD_PARTS: u32 = 200_000;

enum Accepted {
    Plan(RoutingPlan),
    Escalate(RoutingEvaluation),
    Rejected(RoutingFallback),
}

/// Route one message, invoking semantic routing only for an unaddressed desk.
///
/// `explicit_responder` is the host's already-resolved direct mention. A direct
/// conversation supplies its canonical recipient through `fallback_responder`.
/// The primary router is called at most once; the reasoning router is called at
/// most once and only for a well-formed uncertain or high-impact evaluation.
pub async fn route_message(
    primary: Option<&(dyn Router + '_)>,
    reasoning: Option<&(dyn Router + '_)>,
    request: &RoutingRequest,
    explicit_responder: Option<&str>,
    fallback_responder: &str,
) -> RoutingPlan {
    if let Some(id) = explicit_responder {
        return fallback(id, RoutingFallback::ExplicitMention);
    }
    match request.conversation.kind {
        ConversationKind::Direct => {
            return fallback(fallback_responder, RoutingFallback::DirectConversation);
        }
        ConversationKind::General | ConversationKind::Workflow => {
            return fallback(fallback_responder, RoutingFallback::SurfaceRule);
        }
        ConversationKind::Desk => {}
    }
    if request.source != RoutingSource::DeskMessage {
        return fallback(fallback_responder, RoutingFallback::RejectedOutput);
    }
    route_semantic(primary, reasoning, request, fallback_responder).await
}

async fn route_semantic(
    primary: Option<&(dyn Router + '_)>,
    reasoning: Option<&(dyn Router + '_)>,
    request: &RoutingRequest,
    fallback_responder: &str,
) -> RoutingPlan {
    if !request
        .candidates
        .iter()
        .any(|candidate| candidate.available)
    {
        return fallback(fallback_responder, RoutingFallback::NoEligibleCandidate);
    }
    if !valid_request(request) {
        return fallback(fallback_responder, RoutingFallback::RejectedOutput);
    }
    let Some(primary) = primary else {
        return fallback(fallback_responder, RoutingFallback::ProviderUnavailable);
    };
    let Ok(evaluation) = primary.evaluate(request).await else {
        return fallback(fallback_responder, RoutingFallback::ProviderUnavailable);
    };
    match accept(request, evaluation) {
        Accepted::Plan(plan) => plan,
        Accepted::Rejected(reason) => fallback(fallback_responder, reason),
        Accepted::Escalate(mut first) => {
            first.disposition = EvaluationDisposition::EscalationRequired;
            let Some(reasoning) = reasoning else {
                return fallback(fallback_responder, RoutingFallback::EscalationFailed);
            };
            let Ok(evaluation) = reasoning.evaluate(request).await else {
                return fallback(fallback_responder, RoutingFallback::EscalationFailed);
            };
            match accept_final(request, evaluation) {
                Accepted::Plan(plan) => plan,
                Accepted::Escalate(_) | Accepted::Rejected(_) => {
                    fallback(fallback_responder, RoutingFallback::EscalationFailed)
                }
            }
        }
    }
}

/// Route one agent-authored broadcast through the same accepted Choice and
/// bounded `>20%` recipient rule as an unaddressed desk message.
///
/// The request must carry [`RoutingSource::AgentBroadcast`], must name a desk,
/// and must exclude the author from its candidates. Invalid provenance fails
/// to the caller-supplied deterministic destination without invoking a model.
pub async fn route_broadcast(
    primary: Option<&(dyn Router + '_)>,
    reasoning: Option<&(dyn Router + '_)>,
    request: &RoutingRequest,
    fallback_responder: &str,
) -> RoutingPlan {
    let valid_source = match &request.source {
        RoutingSource::AgentBroadcast { author_id } => {
            !author_id.trim().is_empty()
                && !request
                    .candidates
                    .iter()
                    .any(|candidate| candidate.id == *author_id)
        }
        RoutingSource::DeskMessage => false,
    };
    if request.conversation.kind != ConversationKind::Desk || !valid_source {
        return fallback(fallback_responder, RoutingFallback::InvalidBroadcast);
    }
    route_semantic(primary, reasoning, request, fallback_responder).await
}

fn accept(request: &RoutingRequest, evaluation: RoutingEvaluation) -> Accepted {
    accept_inner(request, evaluation, true)
}

fn accept_final(request: &RoutingRequest, evaluation: RoutingEvaluation) -> Accepted {
    accept_inner(request, evaluation, false)
}

fn accept_inner(
    request: &RoutingRequest,
    mut evaluation: RoutingEvaluation,
    may_escalate: bool,
) -> Accepted {
    if evaluation.roster_version != request.roster_version {
        return Accepted::Rejected(RoutingFallback::StaleRoster);
    }
    if !valid_domain(request, &evaluation) {
        evaluation.disposition = EvaluationDisposition::Rejected;
        return Accepted::Rejected(RoutingFallback::RejectedOutput);
    }
    let policy = &request.policy;
    let uncertain = evaluation.confidence < policy.minimum_confidence
        || evaluation.needs_clarification >= policy.clarification_threshold
        || (evaluation.high_impact >= policy.high_impact_threshold
            && evaluation.confidence < policy.high_impact_minimum_confidence);
    if uncertain && may_escalate {
        return Accepted::Escalate(evaluation);
    }
    evaluation.disposition = EvaluationDisposition::Accepted;
    if evaluation.needs_clarification >= policy.clarification_threshold
        || evaluation.primary_responder == "none"
    {
        return Accepted::Plan(RoutingPlan::Clarify { evaluation });
    }
    if uncertain {
        return Accepted::Rejected(RoutingFallback::EscalationFailed);
    }
    compose_plan(request, evaluation)
}

fn valid_request(request: &RoutingRequest) -> bool {
    if request.policy.round_width == 0 || request.policy.choice_option_limit < 2 {
        return false;
    }
    let ids: BTreeSet<_> = request
        .candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    ids.len() == request.candidates.len()
        && request
            .candidates
            .iter()
            .all(|candidate| !candidate.id.trim().is_empty() && candidate.id != "none")
}

fn valid_domain(request: &RoutingRequest, evaluation: &RoutingEvaluation) -> bool {
    let eligible: Vec<_> = request
        .candidates
        .iter()
        .filter(|candidate| candidate.available)
        .collect();
    let mut domain: BTreeSet<&str> = eligible
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    domain.insert("none");
    if domain.len() != eligible.len() + 1
        || evaluation.primary_probabilities.len() != domain.len()
        || evaluation.contributions.len() != eligible.len()
    {
        return false;
    }
    let probability_ids: BTreeSet<_> = evaluation
        .primary_probabilities
        .iter()
        .map(|entry| entry.candidate_id.as_str())
        .collect();
    let contribution_ids: BTreeSet<_> = evaluation
        .contributions
        .iter()
        .map(|entry| entry.candidate_id.as_str())
        .collect();
    let eligible_ids: BTreeSet<_> = eligible
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect();
    let sum = evaluation
        .primary_probabilities
        .iter()
        .try_fold(0_u32, |sum, entry| {
            sum.checked_add(entry.probability.parts())
        });
    let selected = evaluation
        .primary_probabilities
        .iter()
        .find(|entry| entry.candidate_id == evaluation.primary_responder);
    probability_ids == domain
        && contribution_ids == eligible_ids
        && sum == Some(PROBABILITY_SCALE)
        && selected.is_some()
        && !evaluation
            .primary_probabilities
            .iter()
            .any(|entry| selected.is_some_and(|selected| entry.probability > selected.probability))
}

fn compose_plan(request: &RoutingRequest, evaluation: RoutingEvaluation) -> Accepted {
    let policy = &request.policy;
    let primary_id = evaluation.primary_responder.clone();
    if policy.round_width <= 1 {
        return Accepted::Plan(RoutingPlan::One {
            responder_id: primary_id,
            evaluation,
        });
    }
    let mut invited: Vec<_> = evaluation
        .primary_probabilities
        .iter()
        .filter(|entry| {
            entry.candidate_id != primary_id
                && entry.candidate_id != "none"
                && entry.probability.parts() > CONCURRENT_CHOICE_THRESHOLD_PARTS
        })
        .collect();
    invited.sort_by(|left, right| {
        right.probability.cmp(&left.probability).then_with(|| {
            candidate_order(request, &left.candidate_id)
                .cmp(&candidate_order(request, &right.candidate_id))
        })
    });
    let invited_ids = invited
        .into_iter()
        .take(policy.round_width.saturating_sub(1))
        .map(|entry| entry.candidate_id.clone())
        .collect::<Vec<_>>();
    if invited_ids.is_empty() {
        return Accepted::Plan(RoutingPlan::One {
            responder_id: primary_id,
            evaluation,
        });
    }
    Accepted::Plan(RoutingPlan::Hive {
        primary_id,
        invited_ids,
        evaluation,
    })
}

fn candidate_order(request: &RoutingRequest, id: &str) -> usize {
    request
        .candidates
        .iter()
        .position(|candidate| candidate.id == id)
        .unwrap_or(usize::MAX)
}

fn fallback(responder_id: &str, reason: RoutingFallback) -> RoutingPlan {
    RoutingPlan::Fallback {
        responder_id: responder_id.to_owned(),
        reason,
    }
}
