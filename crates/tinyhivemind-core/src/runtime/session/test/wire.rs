//! Wire-shape and trait-contract tests: the serde representation of every
//! session payload type, and the object-safety of the `SessionLog` port.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, assert_wire_round_trip};
use crate::aside::Audience;
use crate::aside::Viewer;

#[test]
fn session_author_variants_pin_their_wire_shape() {
    let authors = [
        (
            SessionAuthor::Operator,
            serde_json::json!({"type":"operator"}),
        ),
        (
            SessionAuthor::Person {
                id: "p1".into(),
                label: "Pat".into(),
            },
            serde_json::json!({"type":"person","id":"p1","label":"Pat"}),
        ),
        (
            SessionAuthor::Agent {
                id: "a1".into(),
                label: "Ada".into(),
            },
            serde_json::json!({"type":"agent","id":"a1","label":"Ada"}),
        ),
        (
            SessionAuthor::System {
                kind: "workflow".into(),
                label: "Build".into(),
            },
            serde_json::json!({"type":"system","kind":"workflow","label":"Build"}),
        ),
    ];
    for (author, expected) in authors {
        assert_wire_round_trip(&author, expected);
    }
}

#[test]
fn session_records_pin_their_wire_shape() {
    assert_wire_round_trip(&Sequence(7), serde_json::json!(7));
    assert_wire_round_trip(
        &Conversation {
            desk_id: "engineering".into(),
            desk_name: "Engineering".into(),
            thread_root: Some(Sequence(4)),
        },
        serde_json::json!({
            "desk_id": "engineering",
            "desk_name": "Engineering",
            "thread_root": 4
        }),
    );

    let raw = LogMessage {
        sequence: Sequence(9),
        chat_id: Some("engineering".into()),
        parent: Some(Sequence(4)),
        author: SessionAuthor::Operator,
        content: "hello".into(),
        audience: Audience::Desk,
    };
    assert_wire_round_trip(
        &raw,
        serde_json::json!({
            "sequence": 9,
            "chat_id": "engineering",
            "parent": 4,
            "author": {"type":"operator"},
            "content": "hello",
            "audience": {"kind": "desk"}
        }),
    );
    assert_wire_round_trip(
        &SessionPage {
            messages: vec![raw],
            next_before: Some(Sequence(9)),
        },
        serde_json::json!({
            "messages": [{
                "sequence": 9,
                "chat_id": "engineering",
                "parent": 4,
                "author": {"type":"operator"},
                "content": "hello",
                "audience": {"kind": "desk"}
            }],
            "next_before": 9
        }),
    );
    assert_wire_round_trip(
        &SessionMessage {
            sequence: Sequence(9),
            author: SessionAuthor::Operator,
            content: "hello".into(),
            audience: Audience::Desk,
            elided: None,
        },
        serde_json::json!({
            "sequence": 9,
            "author": {"type":"operator"},
            "content": "hello",
            "audience": {"kind": "desk"},
            "elided": null
        }),
    );
    assert_wire_round_trip(
        &SessionQuery {
            conversation: Conversation {
                desk_id: "engineering".into(),
                desk_name: "Engineering".into(),
                thread_root: None,
            },
            before: Some(Sequence(10)),
            window: 30,
            viewer: Viewer::Operator,
        },
        serde_json::json!({
            "conversation": {
                "desk_id": "engineering",
                "desk_name": "Engineering",
                "thread_root": null
            },
            "before": 10,
            "window": 30,
            "viewer": {"kind": "operator"}
        }),
    );
    assert_eq!(Sequence(7).to_string(), "7");
}

/// An elided row keeps its sequence, its author and its audience, and carries
/// the range and settlement pointer in place of content.
#[test]
fn an_elided_row_pins_its_wire_shape() {
    assert_wire_round_trip(
        &SessionMessage {
            sequence: Sequence(7),
            author: SessionAuthor::Agent {
                id: "planner".into(),
                label: "Planner".into(),
            },
            content: String::new(),
            audience: Audience::Aside {
                members: vec!["auditor".into()],
            },
            elided: Some(crate::runtime::Elision {
                through: Sequence(10),
                messages: 4,
                settled_at: Some(Sequence(11)),
            }),
        },
        serde_json::json!({
            "sequence": 7,
            "author": {"type": "agent", "id": "planner", "label": "Planner"},
            "content": "",
            "audience": {"kind": "aside", "members": ["auditor"]},
            "elided": {"through": 10, "messages": 4, "settled_at": 11}
        }),
    );
}

#[test]
fn dispatch_scope_canonicalizes_every_general_alias_and_preserves_exact_threads() {
    use crate::{chat::GENERAL_DESK, dispatch::DispatchConversation};

    let aliases = ["", "main", "MAIN", "General", "GENERAL"];
    for alias in aliases {
        for (desk_id, desk_name) in [(alias, "Ordinary label"), ("opaque-id", alias)] {
            let conversation = Conversation {
                desk_id: desk_id.into(),
                desk_name: desk_name.into(),
                thread_root: Some(Sequence(17)),
            };
            assert_eq!(
                DispatchConversation::from(&conversation),
                DispatchConversation {
                    desk_id: GENERAL_DESK.into(),
                    thread_root: Some(17),
                }
            );
        }
    }

    let channel = DispatchConversation::from(&Conversation {
        desk_id: "main".into(),
        desk_name: "General".into(),
        thread_root: None,
    });
    let first_thread = DispatchConversation::from(&Conversation {
        desk_id: "General".into(),
        desk_name: "General".into(),
        thread_root: Some(Sequence(17)),
    });
    let second_thread = DispatchConversation::from(&Conversation {
        desk_id: "MAIN".into(),
        desk_name: "General".into(),
        thread_root: Some(Sequence(18)),
    });
    assert_ne!(channel, first_thread);
    assert_ne!(first_thread, second_thread);

    let named = DispatchConversation::from(&Conversation {
        desk_id: "Engineering".into(),
        desk_name: "engineering".into(),
        thread_root: Some(Sequence(17)),
    });
    assert_eq!(named.desk_id, "Engineering");
}

#[test]
fn session_wire_records_require_every_field() {
    assert!(
        serde_json::from_value::<Conversation>(serde_json::json!({
            "desk_id": "engineering",
            "thread_root": null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<LogMessage>(serde_json::json!({
            "sequence": 1,
            "chat_id": null,
            "parent": null,
            "author": {"type":"operator"}
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SessionPage>(serde_json::json!({
            "next_before": null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SessionMessage>(serde_json::json!({
            "sequence": 1,
            "author": {"type":"operator"}
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SessionQuery>(serde_json::json!({
            "conversation": {
                "desk_id": "engineering",
                "desk_name": "Engineering",
                "thread_root": null
            },
            "before": null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SessionAuthor>(serde_json::json!({
            "type": "person",
            "id": "p1"
        }))
        .is_err()
    );
}

#[test]
fn session_log_is_object_safe() {
    fn accepts_object(_: &dyn SessionLog) {}
    accepts_object(&FakeLog::new(Vec::new()));
}
