//! Locating a change: the first differing leaf, an operation's file, and the
//! review wire.
use super::super::rules::{first_difference, location};
use super::support::*;
use crate::patch::{Operation, Patch, PatchClass};
use serde_json::json;

#[test]
fn the_first_difference_descends_to_the_changed_leaf() {
    let before = json!({"a": {"b": [1, 2, {"c": 3}]}, "z": 1});
    let after = json!({"a": {"b": [1, 2, {"c": 4}]}, "z": 1});
    assert_eq!(first_difference(Some(&before), Some(&after)), "/a/b/2/c");
    assert_eq!(
        first_difference(Some(&json!([1])), Some(&json!([1, 2]))),
        "/1"
    );
    assert_eq!(
        first_difference(Some(&json!({"a/b~": 1})), Some(&json!({}))),
        "/a~1b~0"
    );
    assert_eq!(first_difference(Some(&json!(1)), Some(&json!("1"))), "");
    assert_eq!(first_difference(None, Some(&json!({}))), "");
    assert_eq!(first_difference(Some(&json!({})), Some(&json!({}))), "");
    assert_eq!(first_difference(Some(&json!([])), Some(&json!([]))), "");
}

#[test]
fn an_operation_is_located_in_the_file_it_changes() {
    let json = |pointer: &str| Operation::Json {
        pointer: pointer.into(),
        before: Some(json!({"x": 1})),
        after: Some(json!({"x": 2})),
    };
    assert_eq!(
        location(&json("/manifest/policies")),
        ("hive.json".into(), Some("/policies/x".into()))
    );
    assert_eq!(
        location(&json("/memory/bindings")),
        ("memory.json".into(), Some("/bindings/x".into()))
    );
    let learning = Operation::Learning {
        identity: "notes".into(),
        entry: "e".into(),
        watermark: 0,
        before: None,
        after: Some("x".into()),
    };
    assert_eq!(location(&learning), ("memory.json".into(), None));
    let prompt = Operation::Prompt {
        path: "seats/alice.md".into(),
        before: String::new(),
        after: String::new(),
    };
    assert_eq!(location(&prompt), ("seats/alice.md".into(), None));
}

#[test]
fn pins_the_review_wire() {
    let review = crate::Review {
        changes: vec![Patch {
            class: PatchClass::Prompt,
            operations: vec![],
        }],
        candidate: package(),
    };
    let wire = serde_json::to_value(&review).expect("a review serializes");
    assert_eq!(
        wire["changes"],
        json!([{"class": "prompt", "operations": []}])
    );
    assert_eq!(
        wire.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
        Some(vec!["candidate".to_owned(), "changes".to_owned()])
    );
    let back: crate::Review = serde_json::from_value(wire).expect("a review decodes");
    assert_eq!(back, review);
}
