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
