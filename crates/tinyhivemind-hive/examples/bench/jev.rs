//! Native Jev adapters for routing, weighted consensus, and approval narrowing.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use tinyhivemind_hive::{
    DecisionEvaluation, Sequence, TopicId, TopicProbability,
    approval::Effect,
    responder::{
        CandidateProbability, PROBABILITY_SCALE, Probability, SelectionEvaluation,
        SelectionRequest, Selector, SelectorFuture,
    },
};
use tinyjevclient::{Answer, Choice, Client, EvaluationRequest, Noul, Question, Score};

const STANCE: &str = "Which proposed topic does this worker output support?";
const EVIDENCE: &str = "How strongly is the recommendation supported by evidence?";
const VIOLATION: &str = "Does this output violate an explicit safety or approval constraint?";

/// A [`Selector`] backed by one native Jev Choice request.
#[derive(Clone, Debug)]
pub(crate) struct JevSelector {
    client: Client,
}

impl JevSelector {
    /// Wrap a configured native client.
    pub(crate) const fn new(client: Client) -> Self {
        Self { client }
    }
}

impl Selector for JevSelector {
    fn select<'a>(&'a self, request: &'a SelectionRequest) -> SelectorFuture<'a> {
        Box::pin(async move {
            let criteria = request
                .candidates
                .iter()
                .map(|candidate| {
                    (
                        candidate.id.clone(),
                        Some(json!({
                            "label": candidate.label,
                            "role": candidate.role,
                            "description": candidate.description,
                        })),
                    )
                })
                .collect();
            let evaluation = EvaluationRequest::jev(
                json!({
                    "message": request.message,
                    "desk_id": request.desk_id,
                    "candidates": request.candidates,
                }),
                BTreeMap::from([(
                    "responder".to_owned(),
                    Question::Choice(Choice {
                        instructions: json!("Which candidate is best suited to answer `message`?"),
                        criteria,
                    }),
                )]),
            );
            let result = self
                .client
                .evaluate(&evaluation)
                .await
                .map_err(|error| -> tinyhivemind_hive::BoxError { Box::new(error) })?;
            let Some(Answer::Choice(answer)) = result.response.answers.get("responder") else {
                return Err("Jev response omitted the responder Choice".into());
            };
            let allowed: BTreeSet<String> = request
                .candidates
                .iter()
                .map(|candidate| candidate.id.clone())
                .collect();
            let probabilities = fixed_distribution(&answer.probabilities, &answer.choice, &allowed)
                .map_err(|message| -> tinyhivemind_hive::BoxError { message.into() })?;
            Ok(SelectionEvaluation {
                choice: answer.choice.clone(),
                probabilities: probabilities
                    .into_iter()
                    .map(|(candidate_id, probability)| CandidateProbability {
                        candidate_id,
                        probability,
                    })
                    .collect(),
                confidence: fixed(answer.confidence)
                    .map_err(|message| -> tinyhivemind_hive::BoxError { message.into() })?,
            })
        })
    }
}

/// Build the batched Choice, Score, and Noul request for one worker output.
pub(crate) fn turn_request(state: Value, topics: &[TopicId]) -> EvaluationRequest {
    let abstain = abstention_label(topics);
    let mut criteria: BTreeMap<String, Option<Value>> = topics
        .iter()
        .map(|topic| (topic.to_string(), None))
        .collect();
    criteria.insert(
        abstain,
        Some(json!("the output supports none of the listed topics")),
    );
    EvaluationRequest::jev(
        state,
        BTreeMap::from([
            (
                "stance".to_owned(),
                Question::Choice(Choice {
                    instructions: json!(STANCE),
                    criteria,
                }),
            ),
            (
                "evidence".to_owned(),
                Question::Score(Score {
                    instructions: json!(EVIDENCE),
                    criteria: vec![
                        json!("unsupported opinion"),
                        json!("relevant but indirect evidence"),
                        json!("direct, cited, or decisive evidence"),
                    ],
                }),
            ),
            (
                "violation".to_owned(),
                Question::Noul(Noul {
                    instructions: json!(VIOLATION),
                    criteria: None,
                }),
            ),
        ]),
    )
}

/// Convert typed answers into the pure hive's fixed-point snapshot.
pub(crate) fn decision_from_response(
    response: &tinyjevclient::EvaluationResponse,
    source_sequence: Sequence,
    agent_id: &str,
    topics: &[TopicId],
) -> Result<DecisionEvaluation, String> {
    let abstain = abstention_label(topics);
    let Some(Answer::Choice(stance)) = response.answers.get("stance") else {
        return Err("Jev response omitted stance Choice".to_owned());
    };
    let Some(Answer::Score(evidence)) = response.answers.get("evidence") else {
        return Err("Jev response omitted evidence Score".to_owned());
    };
    let Some(Answer::Noul(violation)) = response.answers.get("violation") else {
        return Err("Jev response omitted violation Noul".to_owned());
    };
    let mut allowed: BTreeSet<String> = topics.iter().map(ToString::to_string).collect();
    allowed.insert(abstain.clone());
    let distribution = fixed_distribution(&stance.probabilities, &stance.choice, &allowed)?;
    let stance = distribution
        .into_iter()
        .map(|(topic, probability)| TopicProbability {
            topic: (topic != abstain).then(|| topic.into()),
            probability,
        })
        .collect();
    Ok(DecisionEvaluation {
        source_sequence,
        agent_id: agent_id.to_owned(),
        stance,
        evidence_quality: fixed(evidence.score / 2.0)?,
        violation_probability: fixed(violation.noul)?,
    })
}

/// Semantic assessment used only to narrow a deterministic approval request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ActionAssessment {
    /// Jev's classified effect.
    pub(crate) effect: Effect,
    /// Choice distribution concentration.
    pub(crate) confidence: Probability,
    /// Normalized severity Score.
    pub(crate) severity: Probability,
    /// Noul probability of a stated policy violation.
    pub(crate) violation: Probability,
}

/// Combine host knowledge and Jev so semantic inference can only narrow.
pub(crate) fn narrow_effect(
    host: Effect,
    assessment: ActionAssessment,
    minimum_confidence: Probability,
    maximum_violation: Probability,
    maximum_severity: Probability,
) -> Effect {
    if assessment.confidence < minimum_confidence
        || assessment.violation > maximum_violation
        || assessment.severity > maximum_severity
        || host == Effect::Unclassified
        || assessment.effect == Effect::Unclassified
    {
        return Effect::Unclassified;
    }
    if host == Effect::Mutating || assessment.effect == Effect::Mutating {
        Effect::Mutating
    } else {
        Effect::ReadOnly
    }
}

fn fixed_distribution(
    distribution: &BTreeMap<String, f64>,
    selected: &str,
    allowed: &BTreeSet<String>,
) -> Result<Vec<(String, Probability)>, String> {
    if distribution.keys().collect::<BTreeSet<_>>() != allowed.iter().collect::<BTreeSet<_>>() {
        return Err("distribution labels do not match the requested alternatives".to_owned());
    }
    let mut fixed: Vec<(String, u32)> = distribution
        .iter()
        .map(|(label, probability)| Ok((label.clone(), fixed(*probability)?.parts())))
        .collect::<Result<_, String>>()?;
    let sum: i64 = fixed.iter().map(|(_, value)| i64::from(*value)).sum();
    let delta = i64::from(PROBABILITY_SCALE) - sum;
    let (_, selected_probability) = fixed
        .iter_mut()
        .find(|(label, _)| label == selected)
        .ok_or_else(|| "selected label is absent from its distribution".to_owned())?;
    let adjusted = i64::from(*selected_probability) + delta;
    *selected_probability = u32::try_from(adjusted)
        .ok()
        .filter(|value| *value <= PROBABILITY_SCALE)
        .ok_or_else(|| "distribution cannot be normalized safely".to_owned())?;
    fixed
        .into_iter()
        .map(|(label, value)| {
            Probability::new(value)
                .map(|probability| (label, probability))
                .ok_or_else(|| "probability exceeds the fixed-point scale".to_owned())
        })
        .collect()
}

fn abstention_label(topics: &[TopicId]) -> String {
    let mut label = "__abstain".to_owned();
    while topics.iter().any(|topic| topic.0 == label) {
        label.push('_');
    }
    label
}

fn fixed(value: f64) -> Result<Probability, String> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err("probability must be finite and between zero and one".to_owned());
    }
    let scaled = (value * f64::from(PROBABILITY_SCALE)).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let parts = scaled as u32;
    Probability::new(parts).ok_or_else(|| "probability exceeds the fixed-point scale".to_owned())
}

#[cfg(test)]
mod test {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn float_distribution_becomes_exact_fixed_point() {
        let distribution =
            BTreeMap::from([("a".to_owned(), 0.333_333_3), ("b".to_owned(), 0.666_666_7)]);
        let allowed = BTreeSet::from(["a".to_owned(), "b".to_owned()]);
        let fixed = fixed_distribution(&distribution, "b", &allowed).expect("converts");
        assert_eq!(
            fixed
                .iter()
                .map(|(_, probability)| probability.parts())
                .sum::<u32>(),
            PROBABILITY_SCALE
        );
    }

    #[test]
    fn distribution_labels_must_match_requested_alternatives() {
        let distribution = BTreeMap::from([("forged".to_owned(), 1.0)]);
        let allowed = BTreeSet::from(["expected".to_owned()]);
        assert!(fixed_distribution(&distribution, "forged", &allowed).is_err());
    }

    #[test]
    fn abstention_label_never_collides_with_a_topic() {
        let topics = [TopicId::from("__abstain")];
        let request = turn_request(json!({}), &topics);
        let Question::Choice(choice) = &request.questions["stance"] else {
            panic!("stance is a Choice");
        };
        assert!(choice.criteria.contains_key("__abstain"));
        assert!(choice.criteria.contains_key("__abstain_"));
    }

    #[test]
    fn semantic_assessment_can_only_preserve_or_raise_risk() {
        let safe = ActionAssessment {
            effect: Effect::ReadOnly,
            confidence: Probability::ONE,
            severity: Probability::ZERO,
            violation: Probability::ZERO,
        };
        assert_eq!(
            narrow_effect(
                Effect::Mutating,
                safe,
                Probability::ONE,
                Probability::ZERO,
                Probability::ZERO,
            ),
            Effect::Mutating
        );
        let risky = ActionAssessment {
            violation: Probability::ONE,
            ..safe
        };
        assert_eq!(
            narrow_effect(
                Effect::ReadOnly,
                risky,
                Probability::ZERO,
                Probability::ZERO,
                Probability::ONE,
            ),
            Effect::Unclassified
        );
    }
}
