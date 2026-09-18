//! Jev question construction, bounded hierarchy, and fixed-point conversion.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use tinyhivemind::responder::{PROBABILITY_SCALE, Probability};
use tinyhivemind_embed::routing::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, Router, RouterFuture,
    RoutingEvaluation, RoutingRequest, RoutingSource,
};

use crate::{
    ChoiceAnswer, Error, NoulAnswer, NoulCriteria, Question, SystemOneAnswer, SystemOneRequest,
    SystemOneResponse, SystemOneTransport,
};

/// Current stable question-schema version recorded in routing evaluations.
const QUESTION_SCHEMA_VERSION: u32 = 2;
const PRIMARY: &str = "primary_responder";
const COLLABORATION: &str = "needs_collaboration";
const CLARIFICATION: &str = "needs_clarification";
const HIGH_IMPACT: &str = "high_impact";

/// A `TypeSafe` Jev semantic router over a host-provided transport.
#[derive(Debug)]
pub struct JevRouter<T> {
    transport: T,
    model: String,
}

impl<T> JevRouter<T> {
    /// Construct a router using `jev-latest`.
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            model: "jev-latest".to_owned(),
        }
    }

    /// Construct a router with an explicitly pinned model identity.
    pub fn with_model(transport: T, model: impl Into<String>) -> Self {
        Self {
            transport,
            model: model.into(),
        }
    }

    /// Borrow the underlying transport.
    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: SystemOneTransport> Router for JevRouter<T> {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        Box::pin(async move {
            if !valid_candidate_ids(request) {
                return Err(boxed(Error::InvalidCandidateIds));
            }
            let eligible: Vec<_> = request
                .candidates
                .iter()
                .filter(|candidate| candidate.available)
                .collect();
            if eligible.is_empty() {
                return Err(boxed(Error::NoEligibleCandidates));
            }
            if request.policy.choice_option_limit < 2 {
                return Err(boxed(Error::InvalidChoiceOptionLimit));
            }
            let state = state(request, &eligible)?;
            if eligible.len() < request.policy.choice_option_limit {
                let wire = SystemOneRequest {
                    state,
                    model: self.model.clone(),
                    questions: normal_questions(request, &eligible),
                };
                let response = self.transport.evaluate(&wire).await.map_err(boxed)?;
                return convert_response(request, &eligible, &response, None);
            }

            let screening = SystemOneRequest {
                state: state.clone(),
                model: self.model.clone(),
                questions: screening_questions(&eligible),
            };
            let screened = self.transport.evaluate(&screening).await.map_err(boxed)?;
            let mut ranked = contributions(&eligible, &screened)?;
            ranked.sort_by(|left, right| {
                right
                    .1
                    .total_cmp(&left.1)
                    .then_with(|| left.0.cmp(&right.0))
            });
            let shortlist_size = request.policy.choice_option_limit.saturating_sub(1);
            let shortlist_ids: BTreeSet<_> = ranked
                .iter()
                .take(shortlist_size)
                .map(|(index, _)| eligible[*index].id.as_str())
                .collect();
            let shortlist: Vec<_> = eligible
                .iter()
                .copied()
                .filter(|candidate| shortlist_ids.contains(candidate.id.as_str()))
                .collect();
            let final_request = SystemOneRequest {
                state,
                model: self.model.clone(),
                questions: BTreeMap::from([(
                    PRIMARY.to_owned(),
                    primary_question(request, &shortlist),
                )]),
            };
            let final_response = self
                .transport
                .evaluate(&final_request)
                .await
                .map_err(boxed)?;
            convert_response(request, &eligible, &final_response, Some(&screened))
        })
    }
}

fn valid_candidate_ids(request: &RoutingRequest) -> bool {
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

fn state(
    request: &RoutingRequest,
    eligible: &[&tinyhivemind_embed::RouteCandidate],
) -> Result<Value, tinyhivemind_embed::routing::RouterError> {
    serde_json::to_value(json!({
        "message": request.message,
        "source": request.source,
        "conversation": request.conversation,
        "desk_purpose": request.desk_purpose,
        "thread_context": request.thread_context,
        "candidates": eligible,
        "roster_version": request.roster_version,
        "routing_policy": request.policy,
    }))
    .map_err(|source| boxed(Error::SerializeState { source }))
}

fn normal_questions(
    request: &RoutingRequest,
    eligible: &[&tinyhivemind_embed::RouteCandidate],
) -> BTreeMap<String, Question> {
    let mut questions = screening_questions(eligible);
    questions.insert(PRIMARY.to_owned(), primary_question(request, eligible));
    questions
}

fn screening_questions(
    eligible: &[&tinyhivemind_embed::RouteCandidate],
) -> BTreeMap<String, Question> {
    let mut questions = BTreeMap::from([
        (
            COLLABORATION.to_owned(),
            noul(
                "Would one competent primary agent be insufficient for `message` without distinct expertise from another candidate?",
                "The request has multiple material domains or benefits from independent specialist challenge",
                "One suitable agent can handle the request",
            ),
        ),
        (
            CLARIFICATION.to_owned(),
            noul(
                "Is essential information absent such that no candidate can be responsibly selected for `message`?",
                "A routing-critical fact must be clarified",
                "The supplied state is sufficient to route",
            ),
        ),
        (
            HIGH_IMPACT.to_owned(),
            noul(
                "Would a materially wrong answer to `message` have unusually serious legal, financial, safety, or irreversible consequences?",
                "The consequences require the higher confidence threshold",
                "Ordinary routing confidence is appropriate",
            ),
        ),
    ]);
    for candidate in eligible {
        questions.insert(
            contribution_key(&candidate.id),
            noul(
                &format!("If collaboration is needed, does candidate `{}` offer distinct expertise relevant to `message` beyond the primary responder?", candidate.id),
                "The candidate adds non-duplicative expertise material to the request",
                "The candidate is irrelevant or duplicates the primary",
            ),
        );
    }
    questions
}

fn primary_question(
    request: &RoutingRequest,
    eligible: &[&tinyhivemind_embed::RouteCandidate],
) -> Question {
    let mut criteria: BTreeMap<String, Option<Value>> = eligible
        .iter()
        .map(|candidate| {
            (
                candidate.id.clone(),
                Some(json!({
                    "label": candidate.label,
                    "role": candidate.role,
                    "description": candidate.description,
                    "capabilities": candidate.capabilities,
                    "learned_topics": candidate.learned_topics,
                })),
            )
        })
        .collect();
    criteria.insert(
        "none".to_owned(),
        Some(json!(
            "No listed candidate is a competent primary responder for this request"
        )),
    );
    let instructions = match &request.source {
        RoutingSource::DeskMessage => {
            "Which eligible candidate is the best primary responder for `message`? Select `none` when no candidate is competent.".to_owned()
        }
        RoutingSource::AgentBroadcast { author_id } => format!(
            "Agent `{author_id}` is handing off `message`. Which eligible teammate is best placed to take up that work? Select `none` when no candidate is competent."
        ),
    };
    Question::Choice {
        instructions: json!(instructions),
        criteria,
    }
}

fn noul(instructions: &str, yes: &str, no: &str) -> Question {
    Question::Noul {
        instructions: json!(instructions),
        criteria: Some(NoulCriteria {
            true_description: yes.to_owned(),
            false_description: no.to_owned(),
        }),
    }
}

fn convert_response(
    request: &RoutingRequest,
    eligible: &[&tinyhivemind_embed::RouteCandidate],
    primary_response: &SystemOneResponse,
    screening_response: Option<&SystemOneResponse>,
) -> Result<RoutingEvaluation, tinyhivemind_embed::routing::RouterError> {
    let screening = screening_response.unwrap_or(primary_response);
    let primary = choice(primary_response, PRIMARY)?;
    let allowed: BTreeSet<_> = eligible
        .iter()
        .map(|candidate| candidate.id.clone())
        .chain(std::iter::once("none".to_owned()))
        .collect();
    let mut fixed = fixed_distribution(&primary.probabilities, &primary.choice)?;
    for missing in allowed.difference(&fixed.keys().cloned().collect()) {
        fixed.insert(missing.clone(), Probability::ZERO);
    }
    if fixed.keys().collect::<BTreeSet<_>>() != allowed.iter().collect::<BTreeSet<_>>() {
        return Err(conversion(
            "primary Choice labels do not match eligible candidates",
        ));
    }
    let primary_probabilities = allowed
        .into_iter()
        .map(|candidate_id| CandidateProbability {
            probability: fixed[&candidate_id],
            candidate_id,
        })
        .collect();
    let contributions = eligible
        .iter()
        .map(|candidate| {
            Ok(ContributionProbability {
                candidate_id: candidate.id.clone(),
                probability: fixed_probability(
                    noul_answer(screening, &contribution_key(&candidate.id))?.noul,
                )?,
            })
        })
        .collect::<Result<_, tinyhivemind_embed::routing::RouterError>>()?;
    Ok(RoutingEvaluation {
        primary_responder: primary.choice.clone(),
        primary_probabilities,
        confidence: fixed_probability(primary.confidence)?,
        needs_collaboration: fixed_probability(noul_answer(screening, COLLABORATION)?.noul)?,
        needs_clarification: fixed_probability(noul_answer(screening, CLARIFICATION)?.noul)?,
        contributions,
        high_impact: fixed_probability(noul_answer(screening, HIGH_IMPACT)?.noul)?,
        model_identity: primary_response.model.clone(),
        question_schema_version: QUESTION_SCHEMA_VERSION,
        roster_version: request.roster_version,
        disposition: EvaluationDisposition::Unchecked,
    })
}

fn contributions(
    eligible: &[&tinyhivemind_embed::RouteCandidate],
    response: &SystemOneResponse,
) -> Result<Vec<(usize, f64)>, tinyhivemind_embed::routing::RouterError> {
    eligible
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            Ok((
                index,
                noul_answer(response, &contribution_key(&candidate.id))?.noul,
            ))
        })
        .collect()
}

fn choice<'a>(
    response: &'a SystemOneResponse,
    id: &str,
) -> Result<&'a ChoiceAnswer, tinyhivemind_embed::routing::RouterError> {
    match response.answers.get(id) {
        Some(SystemOneAnswer::Choice(answer)) => Ok(answer),
        _ => Err(conversion("System One response omitted a required Choice")),
    }
}

fn noul_answer<'a>(
    response: &'a SystemOneResponse,
    id: &str,
) -> Result<&'a NoulAnswer, tinyhivemind_embed::routing::RouterError> {
    match response.answers.get(id) {
        Some(SystemOneAnswer::Noul(answer)) => Ok(answer),
        _ => Err(conversion("System One response omitted a required Noul")),
    }
}

fn contribution_key(id: &str) -> String {
    format!("contributes_{id}")
}

fn fixed_distribution(
    distribution: &BTreeMap<String, f64>,
    selected: &str,
) -> Result<BTreeMap<String, Probability>, tinyhivemind_embed::routing::RouterError> {
    let mut fixed: BTreeMap<String, u32> = distribution
        .iter()
        .map(|(label, value)| Ok((label.clone(), fixed_probability(*value)?.parts())))
        .collect::<Result<_, tinyhivemind_embed::routing::RouterError>>()?;
    let sum: i64 = fixed.values().map(|value| i64::from(*value)).sum();
    let delta = i64::from(PROBABILITY_SCALE) - sum;
    let tolerance = i64::try_from(distribution.len()).unwrap_or(i64::MAX);
    if delta.abs() > tolerance {
        return Err(conversion(
            "Choice distribution total exceeds rounding tolerance",
        ));
    }
    let selected_probability = fixed
        .get_mut(selected)
        .ok_or_else(|| conversion("Choice selection is absent from its distribution"))?;
    let adjusted = i64::from(*selected_probability) + delta;
    *selected_probability = u32::try_from(adjusted)
        .ok()
        .filter(|value| *value <= PROBABILITY_SCALE)
        .ok_or_else(|| conversion("Choice distribution cannot be normalized safely"))?;
    fixed
        .into_iter()
        .map(|(label, parts)| {
            Probability::new(parts)
                .map(|value| (label, value))
                .ok_or_else(|| conversion("probability exceeds the fixed-point scale"))
        })
        .collect()
}

fn fixed_probability(value: f64) -> Result<Probability, tinyhivemind_embed::routing::RouterError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(conversion(
            "probability must be finite and between zero and one",
        ));
    }
    let scaled = (value * f64::from(PROBABILITY_SCALE)).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let parts = scaled as u32;
    Probability::new(parts).ok_or_else(|| conversion("probability exceeds the fixed-point scale"))
}

/// Whether a failed provider response may be retried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryClass {
    /// Rate limit or transient provider overload; apply bounded backoff.
    Retryable,
    /// Authentication, validation, and other failures should not be retried.
    Permanent,
}

/// Classify `TypeSafe`'s documented transient statuses.
#[must_use]
pub const fn classify_retry(status: u16) -> RetryClass {
    match status {
        429 | 529 => RetryClass::Retryable,
        _ => RetryClass::Permanent,
    }
}

fn conversion(message: &'static str) -> tinyhivemind_embed::routing::RouterError {
    boxed(Error::InvalidProviderResponse { message })
}

fn boxed(error: Error) -> tinyhivemind_embed::routing::RouterError {
    Box::new(error)
}
