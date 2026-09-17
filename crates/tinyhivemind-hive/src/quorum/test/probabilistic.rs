//! Probability-weighted support, admission, freshness, and validation.

use super::super::*;
use super::support::{policy, said, standing};
use crate::trace::read;
use tinyhivemind::{Sequence, responder::Probability};

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("test probability is bounded")
}

fn evaluation(sequence: u64, agent: &str, stage: u32, evidence: u32) -> DecisionEvaluation {
    DecisionEvaluation {
        source_sequence: Sequence(sequence),
        agent_id: agent.into(),
        stance: vec![
            TopicProbability {
                topic: Some("stage".into()),
                probability: probability(stage),
            },
            TopicProbability {
                topic: None,
                probability: probability(PROBABILITY_SCALE - stage),
            },
        ],
        evidence_quality: probability(evidence),
        violation_probability: Probability::ZERO,
    }
}

fn transcript() -> Vec<tinyhivemind::SessionMessage> {
    vec![
        said(1, "planner", "!propose #stage Stage it."),
        said(2, "critic", "!support #stage ^1 Bound the blast radius."),
    ]
}

fn admission() -> AdmissionPolicy {
    AdmissionPolicy {
        maximum_violation_probability: probability(100_000),
    }
}

#[test]
fn probabilities_replace_distinct_supporter_count_for_consensus() {
    let transcript = transcript();
    let standings = standings_with_evaluations(
        &read(&transcript),
        &[
            evaluation(1, "planner", 900_000, PROBABILITY_SCALE),
            evaluation(2, "critic", 900_000, PROBABILITY_SCALE),
        ],
        Sequence(2),
        &policy(2),
        &admission(),
    )
    .expect("folds");
    let stage = standing(&standings, "stage");
    assert_eq!(stage.supporters, ["planner", "critic"]);
    assert_eq!(stage.probability_support, 1_800_000);
    assert_eq!(
        consensus(&standings, &policy(2)),
        ConsensusState::Deliberating
    );
}

#[test]
fn evidence_score_scales_a_members_contribution() {
    let transcript = transcript();
    let standings = standings_with_evaluations(
        &read(&transcript),
        &[
            evaluation(1, "planner", PROBABILITY_SCALE, PROBABILITY_SCALE),
            evaluation(2, "critic", PROBABILITY_SCALE, 500_000),
        ],
        Sequence(2),
        &policy(2),
        &admission(),
    )
    .expect("folds");
    assert_eq!(standing(&standings, "stage").probability_support, 1_500_000);
}

#[test]
fn rejected_or_missing_evaluations_contribute_nothing() {
    let transcript = transcript();
    let mut rejected = evaluation(1, "planner", PROBABILITY_SCALE, PROBABILITY_SCALE);
    rejected.violation_probability = probability(100_001);
    let standings = standings_with_evaluations(
        &read(&transcript),
        &[rejected],
        Sequence(2),
        &policy(2),
        &admission(),
    )
    .expect("folds");
    assert_eq!(standing(&standings, "stage").probability_support, 0);
}

#[test]
fn the_latest_evaluation_per_member_replaces_an_earlier_one() {
    let transcript = vec![
        said(1, "planner", "!propose #stage Stage it."),
        said(2, "planner", "!support #stage ^1 Still stage it."),
    ];
    let standings = standings_with_evaluations(
        &read(&transcript),
        &[
            evaluation(1, "planner", PROBABILITY_SCALE, PROBABILITY_SCALE),
            evaluation(2, "planner", 250_000, PROBABILITY_SCALE),
        ],
        Sequence(2),
        &policy(2),
        &admission(),
    )
    .expect("folds");
    assert_eq!(standing(&standings, "stage").probability_support, 250_000);
}

#[test]
fn expired_evaluations_are_ignored_before_source_validation() {
    let transcript = vec![
        said(1, "planner", "!propose #stage Stage it."),
        said(5, "critic", "!support #stage ^1 Bound it."),
    ];
    let mut narrow = policy(2);
    narrow.window = 1;
    let standings = standings_with_evaluations(
        &read(&transcript),
        &[evaluation(
            1,
            "planner",
            PROBABILITY_SCALE,
            PROBABILITY_SCALE,
        )],
        Sequence(5),
        &narrow,
        &admission(),
    )
    .expect("expired evaluations are inert");
    assert_eq!(standing(&standings, "stage").probability_support, 0);
}

#[test]
fn conflicting_duplicate_evaluations_are_rejected() {
    let transcript = transcript();
    assert!(matches!(
        standings_with_evaluations(
            &read(&transcript),
            &[
                evaluation(1, "planner", 800_000, PROBABILITY_SCALE),
                evaluation(1, "planner", 200_000, PROBABILITY_SCALE),
            ],
            Sequence(2),
            &policy(2),
            &admission(),
        ),
        Err(crate::Error::InvalidDecisionDistribution)
    ));
}

#[test]
fn evaluations_cannot_assign_probability_to_unknown_topics() {
    let transcript = transcript();
    let mut unknown = evaluation(1, "planner", 0, PROBABILITY_SCALE);
    unknown.stance = vec![TopicProbability {
        topic: Some("phantom".into()),
        probability: Probability::ONE,
    }];
    assert!(matches!(
        standings_with_evaluations(
            &read(&transcript),
            &[unknown],
            Sequence(2),
            &policy(2),
            &admission(),
        ),
        Err(crate::Error::InvalidDecisionDistribution)
    ));
}

#[test]
fn malformed_and_stale_evaluations_stop_the_fold() {
    let transcript = transcript();
    let mut malformed = evaluation(1, "planner", 500_000, PROBABILITY_SCALE);
    malformed.stance[1].probability = probability(400_000);
    assert!(matches!(
        standings_with_evaluations(
            &read(&transcript),
            &[malformed],
            Sequence(2),
            &policy(2),
            &admission(),
        ),
        Err(crate::Error::InvalidDecisionDistribution)
    ));
    assert!(matches!(
        standings_with_evaluations(
            &read(&transcript),
            &[evaluation(2, "planner", 500_000, PROBABILITY_SCALE)],
            Sequence(2),
            &policy(2),
            &admission(),
        ),
        Err(crate::Error::StaleDecisionEvaluation { .. })
    ));
}
