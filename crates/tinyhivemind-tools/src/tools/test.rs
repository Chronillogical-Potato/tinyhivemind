//! The server's memory: turns, inboxes, and windows.

#![allow(clippy::expect_used)]

use tinyhivemind::speech::{ToolCall, Utterance};

use super::{Dispatch, EpisodeTools, SeatEvent};

fn dispatch() -> Dispatch {
    Dispatch {
        chat: "engineering".into(),
        parent: None,
    }
}

#[test]
fn a_registered_turn_is_visible_until_cleared() {
    let tools = EpisodeTools::new(["lead", "solver"]);
    assert_eq!(tools.open_turn("lead"), None);
    tools.register("lead", dispatch());
    assert_eq!(tools.open_turn("lead"), Some(dispatch()));
    tools.clear("lead");
    assert_eq!(tools.open_turn("lead"), None);
}

#[test]
fn draining_takes_a_seats_calls_in_order_and_leaves_nothing() {
    let tools = EpisodeTools::new(["lead", "solver"]);
    for message in ["first", "second"] {
        tools.record(SeatEvent {
            seat: "lead".into(),
            call: ToolCall::Speak(Utterance::Post {
                message: message.into(),
            }),
            dispatch: dispatch(),
        });
    }
    let drained = tools.drain("lead");
    assert_eq!(drained.len(), 2);
    assert_eq!(
        drained[0].call,
        ToolCall::Speak(Utterance::Post {
            message: "first".into()
        })
    );
    assert!(tools.drain("lead").is_empty(), "drained means gone");
    assert!(
        tools.drain("solver").is_empty(),
        "another seat's inbox is its own"
    );
}

#[test]
fn the_window_is_a_snapshot_and_read_returns_its_newest_rows() {
    let tools = EpisodeTools::new(["lead"]);
    assert!(tools.recent("lead", 5).is_empty(), "no window, no rows");
    tools.window("lead", (1..=5).map(|n| format!("row {n}")).collect());
    assert_eq!(tools.recent("lead", 2), ["row 4", "row 5"]);
    assert_eq!(
        tools.recent("lead", 50).len(),
        5,
        "asking for more returns what there is"
    );
}

#[test]
fn only_listed_seats_are_known() {
    let tools = EpisodeTools::new(["lead"]);
    assert!(tools.knows("lead"));
    assert!(!tools.knows("johnny"));
    assert_eq!(tools.seats(), ["lead"]);
}

// ── the in-process call ─────────────────────────────────────────────────────

fn args(json: serde_json::Value) -> serde_json::Value {
    json
}

#[test]
fn an_in_process_call_is_checked_interpreted_and_recorded() {
    let tools = EpisodeTools::new(["lead", "solver"]);
    tools.register("lead", dispatch());
    let receipt = tools
        .call(
            "lead",
            "complete_episode",
            &args(serde_json::json!({"message": "done", "chat": "engineering", "parent": null})),
        )
        .expect("a well-formed call is accepted");
    assert_eq!(receipt, "recorded: your assignment is complete");
    let drained = tools.drain("lead");
    assert_eq!(drained.len(), 1);
    assert_eq!(
        drained[0].call,
        ToolCall::Speak(Utterance::CompleteEpisode {
            message: "done".into()
        })
    );
    assert_eq!(drained[0].dispatch, dispatch());
}

#[test]
fn an_in_process_call_is_refused_where_the_wire_would_refuse_it() {
    let tools = EpisodeTools::new(["lead", "solver"]);
    let good = |name: &str| {
        args(serde_json::json!({"message": "x", "to": name, "chat": "engineering", "parent": null}))
    };

    // No seat of that name.
    assert!(tools.call("nobody", "post", &good("lead")).is_err());
    // A seat with no open turn.
    let refusal = tools
        .call("lead", "post", &good("lead"))
        .expect_err("no turn is open");
    assert!(refusal.contains("no turn is open"));
    tools.register("lead", dispatch());
    // The wrong thread named.
    let refusal = tools
        .call(
            "lead",
            "post",
            &args(serde_json::json!({"message": "x", "chat": "elsewhere"})),
        )
        .expect_err("wrong chat");
    assert!(refusal.contains("\"chat\": \"engineering\" and \"parent\": null"));
    // A tool the vocabulary has but this server does not serve.
    assert!(tools.call("lead", "dm", &good("solver")).is_err());
    // Asking oneself, and asking a stranger.
    assert!(tools.call("lead", "ask", &good("lead")).is_err());
    let refusal = tools
        .call("lead", "ask", &good("ghost"))
        .expect_err("unknown recipient");
    assert!(refusal.contains("You can ask: lead, solver"));
    // Nothing above was recorded.
    assert!(tools.drain("lead").is_empty());
    // Inside a conversation, `ask` is refused and `complete_episode` is the answer.
    tools.register(
        "lead",
        Dispatch {
            chat: "engineering".into(),
            parent: Some("7".into()),
        },
    );
    let refusal = tools
        .call("lead", "ask", &args(serde_json::json!({"message": "x", "to": "solver", "chat": "engineering", "parent": "7"})))
        .expect_err("no asks inside a conversation");
    assert!(refusal.contains("answer the seat that asked you"));
}

#[test]
fn an_in_process_read_returns_the_window_and_records_nothing() {
    let tools = EpisodeTools::new(["lead"]);
    tools.register("lead", dispatch());
    tools.window("lead", vec!["@operator: one".into(), "@lead: two".into()]);
    let rows = tools
        .call(
            "lead",
            "read",
            &args(serde_json::json!({"limit": 1, "chat": "engineering"})),
        )
        .expect("read is served");
    assert_eq!(rows, "@lead: two");
    assert!(tools.drain("lead").is_empty(), "a read is not an event");
}

/// **A seat already waiting on another may not ask it again.**
///
/// The waiting seat is still turned on the desk -- that turn is what keeps
/// the episode live while the seat it asked is parked -- and its brief is
/// its own unanswered question, a completion refused until the conversation
/// concludes, and "a reply that calls no tool records nothing". With every
/// other door shut, `ask` is the one tool left, and each repeat opens a
/// second conversation with the same seat rather than hurrying the first.
#[test]
fn a_second_ask_to_a_seat_already_being_waited_on_is_refused() {
    let tools = EpisodeTools::new(["lead", "solver", "scribe"]);
    tools.register("lead", dispatch());
    let ask = |to: &str| {
        args(serde_json::json!({
            "message": "what constrains it?", "to": to,
            "chat": "engineering", "parent": null
        }))
    };

    // Nothing outstanding: the first ask is taken.
    tools
        .call("lead", "ask", &ask("solver"))
        .expect("the first ask");
    assert_eq!(tools.drain("lead").len(), 1);

    // The host hands over what the ledger holds before the next turn.
    tools.awaiting("lead", vec!["solver".to_owned()]);
    let refusal = tools
        .call("lead", "ask", &ask("solver"))
        .expect_err("a second ask to the same seat");
    assert!(
        refusal.contains("already asked solver"),
        "the refusal names the seat and why: {refusal}"
    );
    assert!(
        tools.drain("lead").is_empty(),
        "a refused ask records nothing, so no second conversation opens"
    );

    // Another seat is still reachable: waiting on one is not waiting on all.
    tools
        .call("lead", "ask", &ask("scribe"))
        .expect("a different seat");
    assert_eq!(tools.drain("lead").len(), 1);

    // And once the answer lands the host clears it, so the pair is reachable
    // again -- a later episode may need the same two seats talking.
    tools.awaiting("lead", Vec::new());
    tools
        .call("lead", "ask", &ask("solver"))
        .expect("after it concluded");
    assert_eq!(tools.drain("lead").len(), 1);
}

/// **A group ask is one conversation, and is refused whole.**
///
/// The seats named go into one room together, so the acknowledgement says so
/// -- a seat that thinks it opened three conversations will wait for three
/// answers. And an ask that names a seat already being waited on is refused
/// entire, rather than quietly asking the rest: the seat dropped is usually
/// the one the question was about.
#[test]
fn ask_teammates_names_a_group_and_is_refused_whole_if_one_of_them_is_held() {
    let tools = EpisodeTools::new(["lead", "solver", "scribe"]);
    tools.register("lead", dispatch());
    let ask = |to: serde_json::Value| {
        args(serde_json::json!({
            "message": "does this hold for both of you?", "to": to,
            "chat": "engineering", "parent": null
        }))
    };

    let receipt = tools
        .call(
            "lead",
            "ask_teammates",
            &ask(serde_json::json!(["solver", "scribe"])),
        )
        .expect("a group ask");
    assert!(
        receipt.starts_with("your question to solver and scribe is sent:"),
        "the acknowledgement names everyone asked: {receipt}"
    );
    assert!(
        receipt.contains("one conversation together"),
        "and says it is one conversation, not one each: {receipt}"
    );
    assert!(
        receipt.contains("once every one of them has answered"),
        "and what releases the asker: {receipt}"
    );
    assert_eq!(tools.drain("lead").len(), 1, "one ask, one event");

    // One of the two is already being waited on: the whole ask is refused,
    // and the seat that was free is not asked behind the asker's back.
    tools.awaiting("lead", vec!["scribe".to_owned()]);
    let refusal = tools
        .call(
            "lead",
            "ask_teammates",
            &ask(serde_json::json!(["solver", "scribe"])),
        )
        .expect_err("one of them is held");
    assert!(refusal.contains("already asked scribe"), "{refusal}");
    assert!(
        tools.drain("lead").is_empty(),
        "a refused group ask records nothing"
    );

    // A stranger in the group is refused the same way, and named.
    tools.awaiting("lead", Vec::new());
    let refusal = tools
        .call(
            "lead",
            "ask_teammates",
            &ask(serde_json::json!(["solver", "ghost"])),
        )
        .expect_err("a stranger in the group");
    assert!(refusal.contains("@ghost"), "{refusal}");
    assert!(
        refusal.contains("You can ask: lead, scribe, solver"),
        "{refusal}"
    );

    // And a group that names the caller is refused: it cannot ask itself.
    let refusal = tools
        .call(
            "lead",
            "ask_teammates",
            &ask(serde_json::json!(["solver", "lead"])),
        )
        .expect_err("the caller is in the group");
    assert!(refusal.contains("names you"), "{refusal}");
}

#[test]
fn a_named_seat_is_called_by_its_name_in_what_the_caller_reads_back() {
    let tools = EpisodeTools::new(["lead", "solver", "scribe"]);
    tools.name_seats([("solver", "Tess"), ("ghost", "Nobody"), ("scribe", " ")]);
    assert_eq!(tools.display_name("solver"), "Tess");
    assert_eq!(
        tools.display_name("scribe"),
        "scribe",
        "a blank name is none"
    );
    assert_eq!(
        tools.display_name("ghost"),
        "ghost",
        "an unserved seat is not named"
    );
    tools.register("lead", dispatch());
    let ask = |to: &str| {
        args(serde_json::json!({
            "message": "what constrains it?", "to": to,
            "chat": "engineering", "parent": null
        }))
    };
    let receipt = tools.call("lead", "ask", &ask("solver")).expect("asked");
    assert!(
        receipt.starts_with("your question to Tess is sent."),
        "{receipt}"
    );
    assert!(!receipt.contains("@solver"), "{receipt}");
    let refusal = tools
        .call("lead", "ask", &ask("ghost"))
        .expect_err("unknown recipient");
    assert!(
        refusal.contains("You can ask: lead, scribe, Tess (id `solver`)"),
        "{refusal}"
    );
    tools.awaiting("lead", vec!["solver".to_owned()]);
    let refusal = tools
        .call("lead", "ask", &ask("solver"))
        .expect_err("already waiting");
    assert!(refusal.contains("already asked Tess"), "{refusal}");
}

/// **A host may decline a tool its own model cannot file.**
///
/// The vocabulary is otherwise the same everywhere, which is what makes a
/// seat's contract portable. `ask_teammates` opens a conversation with
/// several seats in it, and a host that stores a conversation as a pair
/// cannot represent one -- so it withholds the tool rather than filing the
/// conversation as something it is not.
#[test]
fn a_withheld_tool_is_not_offered_not_contracted_and_not_served() {
    let tools = EpisodeTools::new(["lead", "solver", "scribe"]).withhold(["ask_teammates"]);
    tools.register("lead", dispatch());

    let offered: Vec<String> = tools
        .tool_definitions()
        .into_iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect();
    assert!(offered.contains(&"ask".to_owned()), "{offered:?}");
    assert!(
        !offered.contains(&"ask_teammates".to_owned()),
        "{offered:?}"
    );
    let contracted: Vec<&str> = tools.specs().map(|spec| spec.name).collect();
    assert_eq!(contracted, ["broadcast", "ask", "complete_episode", "read"]);

    let refusal = tools
        .call(
            "lead",
            "ask_teammates",
            &args(serde_json::json!({
                "message": "both of you?", "to": ["solver", "scribe"],
                "chat": "engineering", "parent": null
            })),
        )
        .expect_err("a withheld tool is not served");
    assert_eq!(refusal, "unknown tool ask_teammates");
    assert!(
        tools.drain("lead").is_empty(),
        "and nothing it would have opened was recorded"
    );

    // What the host does serve still works.
    tools
        .call(
            "lead",
            "ask",
            &args(serde_json::json!({
                "message": "you?", "to": "solver",
                "chat": "engineering", "parent": null
            })),
        )
        .expect("ask is served");
    assert_eq!(tools.drain("lead").len(), 1);
}

#[test]
fn an_unnamed_seat_is_called_by_its_id() {
    let tools = EpisodeTools::new(["lead", "solver"]);
    assert_eq!(tools.display_name("solver"), "solver");
    tools.register("lead", dispatch());
    let receipt = tools
        .call(
            "lead",
            "ask",
            &args(serde_json::json!({
                "message": "?", "to": "solver", "chat": "engineering", "parent": null
            })),
        )
        .expect("asked");
    assert!(
        receipt.starts_with("your question to solver is sent."),
        "{receipt}"
    );
}

#[test]
fn the_record_describes_named_recipients_beside_the_ids_a_call_carries() {
    let tools = EpisodeTools::new(["lead", "solver"]);
    let bare = tools.tool_definitions();
    tools.name_seats([("solver", "Tess")]);
    let named = tools.tool_definitions();
    let to = |definitions: &[serde_json::Value]| {
        definitions
            .iter()
            .find(|tool| tool["name"] == "ask")
            .expect("ask is served")["inputSchema"]["properties"]["to"]
            .clone()
    };
    assert_eq!(to(&named)["enum"], serde_json::json!(["lead", "solver"]));
    let description = to(&named)["description"]
        .as_str()
        .expect("described")
        .to_owned();
    assert!(
        description.ends_with("On this desk: lead, Tess (id `solver`)."),
        "{description}"
    );
    assert!(
        !to(&bare)["description"]
            .as_str()
            .expect("described")
            .contains("On this desk")
    );
}
