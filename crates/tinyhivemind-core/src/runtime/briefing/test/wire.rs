//! Wire-shape tests: the serde representation of briefing payload types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use crate::aside::Audience;
use crate::dispatch::MentionDispatchPolicy;
use crate::runtime::{Sequence, SessionAuthor};

#[test]
fn briefing_records_pin_their_wire_shape() {
    let teammate = BriefedTeammate {
        id: "bob".into(),
        label: "Bob".into(),
        role: None,
        description: Some("Reviews changes".into()),
    };
    assert_eq!(
        serde_json::to_value(&teammate).expect("teammate serializes"),
        serde_json::json!({
            "id": "bob",
            "label": "Bob",
            "role": null,
            "description": "Reviews changes"
        })
    );
    assert_eq!(
        serde_json::from_value::<BriefedTeammate>(serde_json::json!({
            "id": "bob",
            "label": "Bob",
            "role": null,
            "description": "Reviews changes"
        }))
        .expect("teammate deserializes"),
        teammate
    );

    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: vec![teammate],
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    let briefing_json = serde_json::json!({
        "viewer_id": "alice",
        "desk_id": "engineering",
        "desk_name": "Engineering",
        "teammates": [{
            "id": "bob",
            "label": "Bob",
            "role": null,
            "description": "Reviews changes"
        }],
        "brevity": { "message_chars": 600, "window": 30 },
        "asides": {
            "enabled": false,
            "max_members": 0,
            "max_messages": 0,
            "must_surface": false,
            "require_thread": false
        }
    });
    assert_eq!(
        serde_json::to_value(&briefing).expect("briefing serializes"),
        briefing_json
    );
    assert_eq!(
        serde_json::from_value::<TeamBriefing>(briefing_json).expect("briefing deserializes"),
        briefing
    );
}

#[test]
fn initialization_pins_its_wire_shape() {
    let briefing = TeamBriefing {
        viewer_id: "alice".into(),
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        teammates: vec![BriefedTeammate {
            id: "bob".into(),
            label: "Bob".into(),
            role: None,
            description: Some("Reviews changes".into()),
        }],
        brevity: BrevityPolicy::DEFAULT,
        asides: AsidePolicy::DEFAULT,
    };
    let initialization = SessionInitialization {
        briefing,
        context: SessionContext {
            threads: vec![crate::runtime::ThreadLine {
                root: Sequence(2),
                opening: "ship the release".into(),
                replies: 1,
                latest: Sequence(3),
                landed: None,
            }],
            pins: Vec::new(),
            notes: vec![BriefingNote {
                heading: "Work raised in this conversation".into(),
                lines: vec!["#12 rewrite the changelog — In review".into()],
            }],
        },
        history: vec![crate::runtime::SessionMessage {
            sequence: Sequence(4),
            author: SessionAuthor::Operator,
            content: "hello".into(),
            audience: Audience::Desk,
            elided: None,
        }],
    };
    let initialization_json = serde_json::json!({
        "briefing": {
            "viewer_id": "alice",
            "desk_id": "engineering",
            "desk_name": "Engineering",
            "teammates": [{
                "id": "bob",
                "label": "Bob",
                "role": null,
                "description": "Reviews changes"
            }],
            "brevity": { "message_chars": 600, "window": 30 },
            "asides": {
                "enabled": false,
                "max_members": 0,
                "max_messages": 0,
                "must_surface": false,
                "require_thread": false
            }
        },
        "context": {
            "threads": [{
                "root": 2,
                "opening": "ship the release",
                "replies": 1,
                "latest": 3,
                "landed": null
            }],
            "pins": [],
            "notes": [{
                "heading": "Work raised in this conversation",
                "lines": ["#12 rewrite the changelog — In review"]
            }]
        },
        "history": [{
            "sequence": 4,
            "author": {"type":"operator"},
            "content": "hello",
            "audience": {"kind": "desk"},
            "elided": null
        }]
    });
    assert_eq!(
        serde_json::to_value(&initialization).expect("initialization serializes"),
        initialization_json
    );
    assert_eq!(
        serde_json::from_value::<SessionInitialization>(initialization_json)
            .expect("initialization deserializes"),
        initialization
    );
}

#[test]
fn briefing_wire_records_require_every_field() {
    assert!(
        serde_json::from_value::<BriefedTeammate>(serde_json::json!({
            "id": "bob",
            "role": null,
            "description": null
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<TeamBriefing>(serde_json::json!({
            "viewer_id": "alice",
            "desk_id": "engineering",
            "desk_name": "Engineering"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SessionInitialization>(serde_json::json!({
            "briefing": {
                "asides": {
                    "enabled": false,
                    "max_members": 0,
                    "max_messages": 0,
                    "must_surface": false,
                    "require_thread": false
                },
                "viewer_id": "alice",
                "desk_id": "engineering",
                "desk_name": "Engineering",
                "teammates": []
            }
        }))
        .is_err()
    );
}

#[test]
fn a_dispatch_context_pins_its_wire_shape_and_requires_every_field() {
    let context = MentionDispatchContext {
        policy: MentionDispatchPolicy {
            enabled: true,
            max_hops: 2,
        },
        hop: 1,
    };
    let context_json = serde_json::json!({
        "policy": { "enabled": true, "max_hops": 2 },
        "hop": 1
    });
    assert_eq!(
        serde_json::to_value(context).expect("context serializes"),
        context_json
    );
    assert_eq!(
        serde_json::from_value::<MentionDispatchContext>(context_json)
            .expect("context deserializes"),
        context
    );
    assert!(
        serde_json::from_value::<MentionDispatchContext>(serde_json::json!({
            "policy": { "enabled": true, "max_hops": 2 }
        }))
        .is_err(),
        "a missing hop must not decode as hop zero, which is the permissive one"
    );
}
