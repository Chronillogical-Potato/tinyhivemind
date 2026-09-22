//! The wire forms: what a host journals and streams, pinned exactly.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::{Value, json};
use tinyhivemind::Sequence;
use tinyhivemind::speech::Utterance;

use crate::conduct::steps::Kind;
use crate::conduct::{Commit, Event, Note, Refusal, Step, Turn};
use crate::driver::Channel;

/// Every field in `required` must be present: a payload missing one fails to
/// decode rather than decoding to something the conductor never said.
fn rejects_missing<T: serde::de::DeserializeOwned>(value: &Value, required: &[&str]) {
    for field in required {
        let mut payload = value.as_object().expect("an object").clone();
        payload.remove(*field);
        assert!(
            serde_json::from_value::<T>(payload.into()).is_err(),
            "missing {field} must be rejected"
        );
    }
}

fn round_trips<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let wire = serde_json::to_value(value).expect("serializes");
    let back: T = serde_json::from_value(wire).expect("deserializes");
    assert_eq!(&back, value);
}

#[test]
fn a_turn_names_its_seat_channel_and_watermark() {
    let thread = Turn {
        seat: "two".into(),
        channel: Channel::Thread {
            root: Sequence(4),
            other: "one".into(),
            opened_it: false,
        },
        since: Some(Sequence(4)),
    };
    let wire = serde_json::to_value(&thread).expect("serializes");
    assert_eq!(
        wire,
        json!({
            "seat": "two",
            "channel": {"kind": "thread", "root": 4, "other": "one", "opened_it": false},
            "since": 4
        })
    );
    rejects_missing::<Turn>(&wire, &["seat", "channel", "since"]);
    round_trips(&thread);

    let desk = Turn {
        seat: "one".into(),
        channel: Channel::Desk,
        since: None,
    };
    let wire = serde_json::to_value(&desk).expect("serializes");
    assert_eq!(wire["channel"], json!({"kind": "desk"}));
    assert_eq!(
        wire["since"],
        json!(null),
        "nothing shown yet is null on the wire, never a sequence"
    );
    round_trips(&desk);
}

#[test]
fn a_note_is_tagged_as_a_step_with_its_fields_beside_the_tag() {
    let note = Step::Note(Note {
        body: "you hold open work".into(),
        thread: None,
        only_for: Some("one".into()),
    });
    assert_eq!(
        serde_json::to_value(&note).expect("serializes"),
        json!({
            "step": "note",
            "body": "you hold open work",
            "thread": null,
            "only_for": "one"
        })
    );
    round_trips(&note);
}

#[test]
fn a_commit_carries_its_conversation_and_an_opaque_purpose() {
    let lifted = Commit {
        author: "two".into(),
        utterance: Utterance::Broadcast {
            message: "three should check the logs".into(),
        },
        thread: None,
        only_for: None,
        conversation: Some(Sequence(4)),
        kind: Kind::Desk,
    };
    let wire = serde_json::to_value(Step::Commit(lifted.clone())).expect("serializes");
    assert_eq!(
        wire,
        json!({
            "step": "commit",
            "author": "two",
            "utterance": {"kind": "broadcast", "message": "three should check the logs"},
            "thread": null,
            "only_for": null,
            "conversation": 4,
            "purpose": {"kind": "desk"}
        })
    );
    let bare = serde_json::to_value(&lifted).expect("serializes");
    rejects_missing::<Commit>(&bare, &["author", "utterance", "purpose"]);
    round_trips(&Step::Commit(lifted));

    for kind in [
        Kind::Thread { root: Sequence(4) },
        Kind::Desk,
        Kind::Conclusion {
            root: Sequence(4),
            forced: true,
        },
        Kind::Discharge,
    ] {
        round_trips(&Commit {
            author: "one".into(),
            utterance: Utterance::CompleteEpisode {
                message: "done".into(),
            },
            thread: Some(Sequence(4)),
            only_for: None,
            conversation: Some(Sequence(4)),
            kind,
        });
    }
}

#[test]
fn an_event_is_tagged_by_kind_inside_its_step() {
    let asked = Step::Event(Event::Asked {
        seat: "one".into(),
        askee: "two".into(),
        root: Sequence(4),
    });
    assert_eq!(
        serde_json::to_value(&asked).expect("serializes"),
        json!({"step": "event", "kind": "asked", "seat": "one", "askee": "two", "root": 4})
    );
    let refused = Event::Refused {
        seat: "one".into(),
        thread: None,
        why: Refusal::AwaitingReply {
            waiting_on: vec!["two".into()],
        },
        at: Sequence(5),
    };
    let wire = serde_json::to_value(&refused).expect("serializes");
    assert_eq!(
        wire,
        json!({
            "kind": "refused",
            "seat": "one",
            "thread": null,
            "why": {"kind": "awaiting_reply", "waiting_on": ["two"]},
            "at": 5
        })
    );
    rejects_missing::<Event>(&wire, &["kind", "seat", "why", "at"]);
}

#[test]
fn every_event_and_refusal_survives_the_wire() {
    let events = [
        Event::Nudged {
            seat: "one".into(),
            thread: Some(Sequence(4)),
        },
        Event::Broadcast {
            seat: "one".into(),
            to: vec!["two".into()],
            at: Sequence(3),
        },
        Event::Unplaced {
            seat: "one".into(),
            at: Sequence(3),
        },
        Event::CompletedByBroadcast {
            seat: "one".into(),
            at: Sequence(3),
        },
        Event::Asked {
            seat: "one".into(),
            askee: "two".into(),
            root: Sequence(4),
        },
        Event::Handoff {
            to: "two".into(),
            from: "one".into(),
            origin: Sequence(3),
        },
        Event::Refused {
            seat: "two".into(),
            thread: Some(Sequence(4)),
            why: Refusal::NotYetShown,
            at: Sequence(6),
        },
        Event::Refused {
            seat: "two".into(),
            thread: None,
            why: Refusal::Undelivered {
                assigned_at: Sequence(5),
            },
            at: Sequence(6),
        },
        Event::Discharged {
            seat: "one".into(),
            at: Sequence(7),
        },
        Event::Concluded {
            root: Sequence(4),
            asker: "one".into(),
            askee: "two".into(),
            forced: false,
            at: Sequence(8),
        },
    ];
    for event in events {
        round_trips(&Step::Event(event.clone()));
        round_trips(&event);
    }
    assert_eq!(
        serde_json::to_value(Refusal::NotYetShown).expect("serializes"),
        json!({"kind": "not_yet_shown"})
    );
}
