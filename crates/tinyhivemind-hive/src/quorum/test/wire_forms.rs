//! Serde wire-form pins for `QuorumPolicy`, `TopicStanding` and
//! `ConsensusState`, plus the shipping default's shape.

use super::super::*;

#[test]
fn a_policy_and_standing_pin_their_wire_forms() {
    let value = serde_json::to_value(QuorumPolicy::DEFAULT).expect("serializes");
    assert_eq!(
        value,
        serde_json::json!({
            "threshold": 2,
            "window": 30,
            "require_grounded": true,
            "refutation_cap": null,
            "require_evidential": false,
        }),
    );
    assert_eq!(
        serde_json::from_value::<QuorumPolicy>(value).expect("deserializes"),
        QuorumPolicy::DEFAULT,
    );

    let standing = TopicStanding {
        topic: "stage".into(),
        supporters: vec!["planner".into()],
        silenced: vec!["scout".into()],
        refuted_by: vec!["auditor".into()],
        support: 1_400,
        probability_support: 750_000,
    };
    let value = serde_json::to_value(&standing).expect("serializes");
    assert_eq!(
        value,
        serde_json::json!({
            "topic": "stage",
            "supporters": ["planner"],
            "silenced": ["scout"],
            "refuted_by": ["auditor"],
            "support": 1_400,
            "probability_support": 750_000,
        }),
    );
    assert_eq!(
        serde_json::from_value::<TopicStanding>(value).expect("deserializes"),
        standing,
    );
}

#[test]
fn consensus_pins_its_tagged_wire_form() {
    assert_eq!(
        serde_json::to_value(ConsensusState::Deliberating).expect("serializes"),
        serde_json::json!({ "state": "deliberating" }),
    );
    assert_eq!(
        serde_json::to_value(ConsensusState::Quorum {
            topic: "stage".into()
        })
        .expect("serializes"),
        serde_json::json!({ "state": "quorum", "topic": "stage" }),
    );
    assert_eq!(
        serde_json::to_value(ConsensusState::Deadlocked {
            topics: vec!["stage".into(), "ship".into()],
        })
        .expect("serializes"),
        serde_json::json!({ "state": "deadlocked", "topics": ["stage", "ship"] }),
    );
}

#[test]
fn the_default_policy_is_the_conservative_one() {
    assert_eq!(QuorumPolicy::default(), QuorumPolicy::DEFAULT);
    assert_eq!(QuorumPolicy::default().threshold, 2);
    // Both narrowing knobs are off by default, because the benchmark scored
    // them and they lost. See `docs/experiments/`.
    assert_eq!(QuorumPolicy::default().refutation_cap, None);
    assert!(!QuorumPolicy::default().require_evidential);
}
