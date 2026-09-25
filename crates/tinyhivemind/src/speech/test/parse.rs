//! Reading a tool call, and refusing one to the seat that made it.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::support::{members, post};
use crate::speech::{
    CallArguments, READ_DEFAULT, READ_MAX, ToolCall, Utterance, UtteranceRejection,
    check_recipients, interpret, read_limit,
};
use tinyhivemind_core::roster::Roster;

fn said(name: &str, arguments: &CallArguments<'_>) -> Utterance {
    match interpret(name, arguments).expect("a well-formed call") {
        ToolCall::Speak(utterance) => utterance,
        ToolCall::Read { limit } => panic!("expected speech, got a read of {limit}"),
    }
}

#[test]
fn a_post_is_one_utterance_with_its_message_trimmed() {
    assert_eq!(
        said(
            "post",
            &CallArguments {
                message: Some("  B holds at 10^18  "),
                ..Default::default()
            }
        ),
        post("B holds at 10^18"),
    );
}

#[test]
fn complete_episode_carries_its_message_and_says_the_agents_work_is_finished() {
    let utterance = said(
        "complete_episode",
        &CallArguments {
            message: Some("Psi(10^18) = 62418970; checker signed off"),
            ..Default::default()
        },
    );
    assert!(utterance.closing());
    assert_eq!(
        utterance.message(),
        "Psi(10^18) = 62418970; checker signed off"
    );
    assert!(utterance.completes_episode());
    assert!(!post("anything").closing(), "only completion closes");
    assert!(!post("anything").completes_episode());
}

#[test]
fn broadcast_carries_work_for_semantic_routing() {
    let utterance = said(
        "broadcast",
        &CallArguments {
            message: Some("Prove the recurrence independently"),
            ..Default::default()
        },
    );
    assert!(utterance.broadcasting());
    assert_eq!(utterance.message(), "Prove the recurrence independently");
}

#[test]
fn a_dm_strips_the_at_and_keeps_the_order_it_was_written() {
    let to = ["@checker".to_string(), " theory ".to_string()];
    assert_eq!(
        said(
            "dm",
            &CallArguments {
                message: Some("recheck the depth"),
                to: &to,
                ..Default::default()
            }
        ),
        Utterance::Dm {
            to: vec!["checker".into(), "theory".into()],
            message: "recheck the depth".into(),
        },
    );
}

#[test]
fn a_dm_naming_the_same_seat_twice_names_it_once() {
    let to = ["checker".to_string(), "@checker".to_string()];
    let Utterance::Dm { to: named, .. } = said(
        "dm",
        &CallArguments {
            message: Some("again"),
            to: &to,
            ..Default::default()
        },
    ) else {
        panic!("a dm");
    };
    assert_eq!(named, vec!["checker".to_string()]);
}

#[test]
fn refuses_a_message_that_is_absent_or_only_whitespace() {
    for message in [None, Some(""), Some("   \n ")] {
        for tool in ["post", "complete_episode", "broadcast", "dm", "ask"] {
            let to = ["checker".to_string()];
            assert_eq!(
                interpret(
                    tool,
                    &CallArguments {
                        message,
                        to: &to,
                        ..Default::default()
                    }
                ),
                Err(UtteranceRejection::EmptyText { field: "message" }),
                "{tool} with {message:?}",
            );
        }
    }
}

#[test]
fn legacy_close_is_still_accepted_but_not_advertised() {
    let utterance = said(
        "close",
        &CallArguments {
            message: Some("done"),
            ..Default::default()
        },
    );
    assert!(utterance.completes_episode());
}

#[test]
fn refuses_a_dm_that_names_nobody() {
    let blanks = [" ".to_string(), "@".to_string()];
    for to in [&[][..], &blanks[..]] {
        assert_eq!(
            interpret(
                "dm",
                &CallArguments {
                    message: Some("hi"),
                    to,
                    ..Default::default()
                }
            ),
            Err(UtteranceRejection::NoRecipients),
            "{to:?}",
        );
    }
}

#[test]
fn an_ask_names_one_seat_and_keeps_its_question() {
    let to = ["@checker".to_string()];
    let utterance = said(
        "ask",
        &CallArguments {
            message: Some("  is the depth bound tight?  "),
            to: &to,
            ..Default::default()
        },
    );
    assert_eq!(
        utterance,
        Utterance::Ask {
            to: vec!["checker".into()],
            message: "is the depth bound tight?".into(),
        },
    );
    assert_eq!(utterance.asks(), ["checker".to_string()]);
    assert!(!utterance.completes_episode(), "asking is not finishing");
    assert!(!utterance.broadcasting());
    assert!(post("anything").asks().is_empty(), "only an ask asks");
}

#[test]
fn refuses_an_ask_that_names_nobody() {
    let blanks = [" ".to_string(), "@".to_string()];
    for to in [&[][..], &blanks[..]] {
        assert_eq!(
            interpret(
                "ask",
                &CallArguments {
                    message: Some("anyone?"),
                    to,
                    ..Default::default()
                }
            ),
            Err(UtteranceRejection::NoRecipients),
            "{to:?}",
        );
    }
}

#[test]
fn ask_teammates_names_a_group_and_keeps_them_in_order() {
    let to = ["@checker".to_string(), "theory".to_string()];
    assert_eq!(
        interpret(
            "ask_teammates",
            &CallArguments {
                message: Some("which of you?"),
                to: &to,
                ..Default::default()
            }
        ),
        Ok(ToolCall::Speak(Utterance::Ask {
            to: vec!["checker".into(), "theory".into()],
            message: "which of you?".into(),
        })),
        "a group question is the same utterance, with the group in it",
    );
}

#[test]
fn each_asking_tool_refuses_the_others_arity_and_names_it() {
    let two = ["checker".to_string(), "theory".to_string()];
    assert_eq!(
        interpret(
            "ask",
            &CallArguments {
                message: Some("both of you?"),
                to: &two,
                ..Default::default()
            }
        ),
        Err(UtteranceRejection::OneRecipient { count: 2 }),
        "`ask` is one seat, and the refusal says which tool takes two",
    );
    let one = ["checker".to_string()];
    assert_eq!(
        interpret(
            "ask_teammates",
            &CallArguments {
                message: Some("just you?"),
                to: &one,
                ..Default::default()
            }
        ),
        Err(UtteranceRejection::NotAGroup),
        "a room of one is a pair, and the refusal says so",
    );
    for tool in ["ask", "ask_teammates"] {
        assert_eq!(
            interpret(
                tool,
                &CallArguments {
                    message: Some("anyone?"),
                    to: &[],
                    ..Default::default()
                }
            ),
            Err(UtteranceRejection::NoRecipients),
            "{tool}",
        );
    }
}

#[test]
fn an_ask_naming_a_seat_twice_asks_it_once() {
    let to = ["checker".to_string(), "@checker".to_string()];
    assert_eq!(
        interpret(
            "ask",
            &CallArguments {
                message: Some("twice?"),
                to: &to,
                ..Default::default()
            }
        ),
        Ok(ToolCall::Speak(Utterance::Ask {
            to: vec!["checker".into()],
            message: "twice?".into(),
        })),
        "a seat named twice is one seat in the conversation",
    );
}

#[test]
fn refuses_an_ask_to_a_seat_the_roster_does_not_hold() {
    let members = members();
    let roster = Roster::new(&members, &[], &[]);
    let utterance = Utterance::Ask {
        to: vec!["johnny".into()],
        message: "are you there?".into(),
    };
    assert_eq!(
        check_recipients(&utterance, "solver", &roster),
        Err(UtteranceRejection::UnknownRecipient {
            id: "johnny".into()
        }),
    );
}

#[test]
fn refuses_an_ask_a_seat_addressed_to_itself() {
    let members = members();
    let roster = Roster::new(&members, &[], &[]);
    let alone = Utterance::Ask {
        to: vec!["solver".into()],
        message: "what do I think?".into(),
    };
    assert_eq!(
        check_recipients(&alone, "solver", &roster),
        Err(UtteranceRejection::SelfRecipient),
    );
    let peer = Utterance::Ask {
        to: vec!["checker".into()],
        message: "what do you think?".into(),
    };
    assert_eq!(check_recipients(&peer, "solver", &roster), Ok(()));
    let group = Utterance::Ask {
        to: vec!["checker".into(), "johnny".into()],
        message: "what do you two think?".into(),
    };
    assert_eq!(
        check_recipients(&group, "solver", &roster),
        Err(UtteranceRejection::UnknownRecipient {
            id: "johnny".into()
        }),
        "a group ask is refused whole where one of the seats is a stranger",
    );
}

#[test]
fn refuses_a_tool_the_room_does_not_serve() {
    assert_eq!(
        interpret("shout", &CallArguments::default()),
        Err(UtteranceRejection::UnknownTool {
            name: "shout".into()
        }),
    );
}

#[test]
fn a_rejection_reads_as_a_sentence_the_seat_can_act_on() {
    assert_eq!(
        UtteranceRejection::EmptyText { field: "message" }.to_string(),
        "`message` must be a non-empty string",
    );
    assert_eq!(
        UtteranceRejection::NoRecipients.to_string(),
        "`to` must name at least one seat",
    );
    assert_eq!(
        UtteranceRejection::UnknownTool {
            name: "shout".into()
        }
        .to_string(),
        "unknown tool shout",
    );
    assert_eq!(
        UtteranceRejection::UnknownRecipient {
            id: "johnny".into()
        }
        .to_string(),
        "`to` names @johnny, who is not an active seat on this desk",
    );
    assert_eq!(
        UtteranceRejection::SelfRecipient.to_string(),
        "`to` names you; a message to yourself reaches nobody else",
    );
}

#[test]
fn a_read_is_clamped_rather_than_taken_at_its_word() {
    assert_eq!(read_limit(None), READ_DEFAULT);
    assert_eq!(read_limit(Some(0)), 1);
    assert_eq!(read_limit(Some(7)), 7);
    assert_eq!(read_limit(Some(10_000)), READ_MAX);
    assert_eq!(read_limit(Some(u64::MAX)), READ_MAX);
    assert_eq!(
        interpret(
            "read",
            &CallArguments {
                limit: Some(10_000),
                ..Default::default()
            }
        ),
        Ok(ToolCall::Read { limit: READ_MAX }),
    );
}

#[test]
fn refuses_a_dm_to_a_seat_the_roster_does_not_hold() {
    let members = members();
    let roster = Roster::new(&members, &[], &[]);
    let utterance = Utterance::Dm {
        to: vec!["checker".into(), "johnny".into()],
        message: "recheck".into(),
    };
    assert_eq!(
        check_recipients(&utterance, "solver", &roster),
        Err(UtteranceRejection::UnknownRecipient {
            id: "johnny".into()
        }),
        "the first unresolvable id stops the check rather than being dropped",
    );
}

#[test]
fn refuses_a_dm_a_seat_addressed_only_to_itself() {
    let members = members();
    let roster = Roster::new(&members, &[], &[]);
    let alone = Utterance::Dm {
        to: vec!["solver".into()],
        message: "thinking out loud".into(),
    };
    assert_eq!(
        check_recipients(&alone, "solver", &roster),
        Err(UtteranceRejection::SelfRecipient),
    );
    let with_a_peer = Utterance::Dm {
        to: vec!["solver".into(), "checker".into()],
        message: "thinking out loud".into(),
    };
    assert_eq!(
        check_recipients(&with_a_peer, "solver", &roster),
        Ok(()),
        "naming itself alongside a peer is a peer message",
    );
    assert_eq!(
        check_recipients(&post("to nobody in particular"), "solver", &roster),
        Ok(()),
        "a post has no recipients to check",
    );
}

#[test]
fn the_wire_form_is_what_a_host_writes_and_reads_back() {
    let cases = [
        (
            post("B holds"),
            serde_json::json!({ "kind": "post", "message": "B holds" }),
        ),
        (
            Utterance::Broadcast {
                message: "verify this".into(),
            },
            serde_json::json!({ "kind": "broadcast", "message": "verify this" }),
        ),
        (
            Utterance::Dm {
                to: vec!["checker".into()],
                message: "recheck".into(),
            },
            serde_json::json!({ "kind": "dm", "to": ["checker"], "message": "recheck" }),
        ),
        (
            Utterance::Ask {
                to: vec!["checker".into(), "theory".into()],
                message: "is it tight?".into(),
            },
            serde_json::json!({
                "kind": "ask",
                "to": ["checker", "theory"],
                "message": "is it tight?",
            }),
        ),
        (
            Utterance::CompleteEpisode {
                message: "delivered".into(),
            },
            serde_json::json!({ "kind": "complete_episode", "message": "delivered" }),
        ),
    ];
    for (utterance, wire) in cases {
        assert_eq!(serde_json::to_value(&utterance).expect("serializes"), wire);
        assert_eq!(
            serde_json::from_value::<Utterance>(wire.clone()).expect("round trips"),
            utterance,
        );
    }
    assert_eq!(
        serde_json::from_value::<Utterance>(
            serde_json::json!({"kind":"close", "message":"legacy"})
        )
        .expect("legacy close rows remain readable"),
        Utterance::CompleteEpisode {
            message: "legacy".into()
        }
    );
}
