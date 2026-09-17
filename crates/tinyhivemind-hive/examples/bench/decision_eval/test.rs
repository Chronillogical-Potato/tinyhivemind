//! Deterministic corpus, schema, metrics, and escaping tests.

#![allow(clippy::expect_used, clippy::float_cmp)]

use super::*;
use tinyjevclient::Question;

#[test]
fn cases_cycle_deterministically_with_explicit_truth() {
    assert_eq!(Case::at(0).route, "reviewer");
    assert_eq!(Case::at(1).evidence, 1);
    assert!(Case::at(3).violation);
    assert_eq!(Case::at(6).route, Case::at(0).route);
}

#[test]
fn request_batches_all_three_independent_primitives() {
    let request = Case::at(0).request();
    assert!(matches!(request.questions["stance"], Question::Choice(_)));
    assert!(matches!(request.questions["route"], Question::Choice(_)));
    assert!(matches!(request.questions["evidence"], Question::Score(_)));
    assert!(matches!(request.questions["violation"], Question::Noul(_)));
    request.validate().expect("valid benchmark request");
}

#[test]
fn strict_schema_requires_every_answer_and_distribution_member() {
    let request = Case::at(0).request();
    let schema = response_schema(&request);
    assert_eq!(schema["required"], json!(["answers"]));
    assert_eq!(
        schema["properties"]["answers"]["required"],
        json!(["evidence", "route", "stance", "violation"])
    );
    assert_eq!(
        schema["properties"]["answers"]["properties"]["stance"]["properties"]["probabilities"]["additionalProperties"],
        false
    );
}

#[test]
fn percentile_and_delta_helpers_are_total() {
    let aggregate = Aggregate {
        latencies: vec![4.0, 1.0, 3.0, 2.0],
        ..Aggregate::default()
    };
    assert_eq!(aggregate.p50(), 2.0);
    assert_eq!(aggregate.p99(), 3.0);
    assert_eq!(divide(1.0, 0.0), 0.0);
    assert_eq!(savings(0.0, 1.0), 0.0);
}

#[test]
fn curl_config_escaping_covers_secrets_and_json_control_characters() {
    assert_eq!(escape("a\\\"\n\r"), "a\\\\\\\"\\n\\r");
}

#[test]
fn hybrid_parallelism_is_bounded_by_jobs_and_jev_capacity() {
    assert_eq!(hybrid_parallelism(0), 1);
    assert_eq!(hybrid_parallelism(1), 1);
    assert_eq!(hybrid_parallelism(2), 2);
    assert_eq!(hybrid_parallelism(32), JEV_MAX_IN_FLIGHT as u64);
}
