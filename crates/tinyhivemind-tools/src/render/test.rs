//! The rendering is the vocabulary, verbatim, plus the two thread arguments.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::json;
use tinyhivemind::speech::tool_specs;

use super::{Arguments, arguments, serves, tool_definitions};

fn names(seats: &[String]) -> Vec<String> {
    tool_definitions(seats)
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn serves_the_vocabulary_minus_post_and_dm_in_its_order() {
    assert_eq!(
        names(&[]),
        [
            "broadcast",
            "ask",
            "ask_teammates",
            "complete_episode",
            "read"
        ]
    );
    assert!(!serves("dm"));
    assert!(!serves("post"));
    assert!(serves("ask"));
    assert_eq!(
        tool_specs().len(),
        names(&[]).len() + 2,
        "exactly two tools are withheld"
    );
}

#[test]
fn descriptions_are_the_specs_own_words() {
    for tool in tool_definitions(&[]) {
        let spec = tool_specs()
            .iter()
            .find(|spec| spec.name == tool["name"].as_str().unwrap())
            .expect("served tools are specs");
        assert_eq!(tool["description"].as_str().unwrap(), spec.description);
    }
}

#[test]
fn every_tool_takes_chat_and_optionally_parent() {
    for tool in tool_definitions(&[]) {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["properties"]["chat"]["type"], "string");
        assert_eq!(
            schema["properties"]["parent"]["type"],
            json!(["string", "null"])
        );
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("chat")), "{}", tool["name"]);
        assert!(!required.contains(&json!("parent")), "{}", tool["name"]);
    }
}

#[test]
fn both_asking_tools_offer_the_seats_as_their_choices() {
    let seats = ["lead".to_string(), "solver".to_string()];
    let definitions = tool_definitions(&seats);
    let of = |name: &str| {
        definitions
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("the tool is served")["inputSchema"]["properties"]["to"]
            .clone()
    };
    // `ask` is one seat: the choices constrain the value itself.
    let ask = of("ask");
    assert_eq!(ask["type"], "string");
    assert_eq!(ask["enum"], json!(["lead", "solver"]));
    // `ask_teammates` is a list: they constrain each entry.
    let group = of("ask_teammates");
    assert_eq!(group["type"], "array");
    assert_eq!(group["items"]["type"], "string");
    assert_eq!(
        group["items"]["enum"],
        json!(["lead", "solver"]),
        "the choices constrain each seat named, since `to` is a list"
    );
}

#[test]
fn read_renders_its_bounds_as_the_schema_says() {
    let read = tool_definitions(&[])
        .into_iter()
        .find(|tool| tool["name"] == "read")
        .unwrap();
    let limit = &read["inputSchema"]["properties"]["limit"];
    assert_eq!(limit["type"], "integer");
    assert_eq!(limit["minimum"], 1);
    assert_eq!(limit["maximum"], 100);
    assert_eq!(limit["default"], 20);
    assert!(
        !read["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("limit"))
    );
}

#[test]
fn arguments_are_read_from_every_shape_a_dispatcher_sends() {
    let expected = Arguments {
        message: Some("hi".into()),
        to: vec!["solver".into()],
        limit: Some(3),
        chat: Some("eng".into()),
        parent: Some("42".into()),
    };
    let object = json!({ "name": "ask", "arguments": {
        "message": "hi", "to": "solver", "limit": 3, "chat": "eng", "parent": "42" } });
    let string = json!({ "name": "ask", "arguments":
        "{\"message\":\"hi\",\"to\":\"solver\",\"limit\":3,\"chat\":\"eng\",\"parent\":\"42\"}" });
    let flat = json!({ "name": "ask",
        "message": "hi", "to": "solver", "limit": 3, "chat": "eng", "parent": "42" });
    for params in [object, string, flat] {
        assert_eq!(arguments(&params), expected, "{params}");
    }
}

#[test]
fn to_is_read_as_one_seat_or_several() {
    let one = arguments(&json!({ "arguments": { "to": "solver" } }));
    assert_eq!(one.to, ["solver"]);
    let many = arguments(&json!({ "arguments": { "to": ["solver", "checker", 7] } }));
    assert_eq!(
        many.to,
        ["solver", "checker"],
        "a non-string entry is dropped here, not refused"
    );
    let none = arguments(&json!({ "arguments": { "to": 7 } }));
    assert!(none.to.is_empty());
    assert_eq!(
        arguments(&json!({ "arguments": { "parent": null } })).parent,
        None
    );
}

#[test]
fn a_call_borrows_as_the_algebras_shape() {
    let owned = Arguments {
        message: Some("m".into()),
        to: vec!["a".into()],
        limit: Some(2),
        ..Default::default()
    };
    let call = owned.call();
    assert_eq!(call.message, Some("m"));
    assert_eq!(call.to, ["a"]);
    assert_eq!(call.limit, Some(2));
}
