//! The tool surface every host renders.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use crate::speech::{CallArguments, ParameterKind, READ_DEFAULT, READ_MAX, interpret, tool_specs};

#[test]
fn serves_exactly_the_seven_tools_a_seat_may_call() {
    let names: Vec<&str> = tool_specs().iter().map(|spec| spec.name).collect();
    assert_eq!(
        names,
        vec![
            "post",
            "broadcast",
            "dm",
            "ask",
            "ask_teammates",
            "complete_episode",
            "read"
        ]
    );
}

#[test]
fn an_ask_takes_one_seat_and_says_the_answer_comes_later() {
    let ask = tool_specs()
        .iter()
        .find(|spec| spec.name == "ask")
        .expect("ask is served");
    let to = ask.parameters.first().expect("ask takes a seat");
    assert_eq!(to.name, "to");
    assert_eq!(
        to.kind,
        ParameterKind::Text,
        "one seat, so the schema says one string rather than a list",
    );
    assert!(to.required);
    assert!(
        ask.description.contains("`ask_teammates`"),
        "a seat asking two is told which tool puts them in one room",
    );
    assert!(
        ask.description.contains("later turn"),
        "a seat is told the answer does not arrive while it waits",
    );
    assert!(
        ask.description.contains("not be able to finish"),
        "a seat is told an open question holds its completion",
    );
    assert!(
        ask.description.contains("not a handoff"),
        "a seat is told the work stays its own",
    );
}

#[test]
fn a_group_ask_takes_two_or_more_and_says_what_a_shared_room_is_for() {
    let group = tool_specs()
        .iter()
        .find(|spec| spec.name == "ask_teammates")
        .expect("ask_teammates is served");
    let to = group.parameters.first().expect("it takes seats");
    assert_eq!(to.name, "to");
    assert_eq!(
        to.kind,
        ParameterKind::TextList,
        "a group is a list of ids, and the schema says so",
    );
    assert!(
        group.description.contains("read each other's answers"),
        "a seat is told what a shared conversation buys it",
    );
    assert!(
        group.description.contains("`ask`"),
        "and which tool takes a question only one seat can settle",
    );
}

#[test]
fn every_tool_is_one_a_seat_can_actually_call() {
    for spec in tool_specs() {
        // Two, so the group tool has a group; `ask` takes the first of them.
        let to = ["checker".to_string(), "theory".to_string()];
        let to = if spec.name == "ask" {
            &to[..1]
        } else {
            &to[..]
        };
        let arguments = CallArguments {
            message: Some("something"),
            to,
            limit: None,
        };
        assert!(
            interpret(spec.name, &arguments).is_ok(),
            "{} is served but not interpreted",
            spec.name,
        );
    }
}

#[test]
fn every_tool_says_what_it_is_for_and_what_it_takes() {
    for spec in tool_specs() {
        assert!(
            spec.description.len() > 80,
            "{} needs a description a seat can act on",
            spec.name,
        );
        for parameter in spec.parameters {
            assert!(
                !parameter.name.is_empty(),
                "{} has a nameless argument",
                spec.name
            );
        }
    }
}

#[test]
fn the_read_bounds_are_the_ones_the_room_enforces() {
    let read = tool_specs()
        .iter()
        .find(|spec| spec.name == "read")
        .expect("read is served");
    let limit = read.parameters.first().expect("read takes a limit");
    assert_eq!(
        limit.kind,
        ParameterKind::Count {
            default: READ_DEFAULT as u64,
            min: 1,
            max: READ_MAX as u64,
        },
        "the schema a seat reads must state the clamp the room applies",
    );
    assert!(!limit.required, "a read without a limit is still a read");
}

#[test]
fn speaking_is_described_as_the_only_way_to_be_heard() {
    let post = tool_specs()
        .iter()
        .find(|spec| spec.name == "post")
        .expect("post is served");
    assert!(
        post.description.contains("reaches nobody"),
        "a seat is told, here and nowhere else, that its free text is not speech",
    );
    let complete = tool_specs()
        .iter()
        .find(|spec| spec.name == "complete_episode")
        .expect("completion is served");
    assert!(
        complete
            .description
            .contains("that message is your finding"),
        "a seat is told its completion message is how a fact reaches the desk",
    );
    assert!(tool_specs().iter().any(|spec| spec.name == "broadcast"));
    assert!(!tool_specs().iter().any(|spec| spec.name == "close"));
}

#[test]
fn what_a_seat_commits_is_described_as_read_by_a_person() {
    for name in ["post", "broadcast", "complete_episode"] {
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == name)
            .expect("the tool is served");
        assert!(
            spec.description.contains("A person reads this"),
            "`{name}` says a person reads what it carries",
        );
        assert!(
            spec.description.contains("by name"),
            "`{name}` asks for teammates by name",
        );
    }
    let complete = tool_specs()
        .iter()
        .find(|spec| spec.name == "complete_episode")
        .expect("completion is served");
    let message = complete
        .parameters
        .iter()
        .find(|parameter| parameter.name == "message")
        .and_then(|parameter| parameter.description)
        .expect("the finding is described");
    assert!(message.contains("for a person"));
    assert!(!message.contains("nothing else you wrote"));
}

#[test]
fn a_recipient_is_named_by_id_not_by_display_name() {
    for name in ["ask", "dm"] {
        let to = tool_specs()
            .iter()
            .find(|spec| spec.name == name)
            .and_then(|spec| spec.parameters.iter().find(|p| p.name == "to"))
            .and_then(|parameter| parameter.description)
            .expect("the recipient is described");
        assert!(to.contains("id"), "`{name}` asks for an id");
        assert!(to.contains("name"), "`{name}` says a name is not an id");
    }
}
