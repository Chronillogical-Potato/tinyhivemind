//! The brief says what the episode knows, and only that.

#![allow(clippy::expect_used)]

use tinyhivemind::Sequence;
use tinyhivemind::speech::{Utterance, tool_specs};

use super::{Channel, ConversationView, EpisodeBrief, standing_contract};
use crate::driver::test::{committed, episode, hive};
use crate::driver::{CompletionDriver, DriverState};

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

fn state_with_an_ask() -> (crate::OpenHumanHive, DriverState) {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let asked = run(driver.apply_committed(
        &state,
        committed(
            "one",
            1,
            Utterance::Ask {
                to: "two".into(),
                message: "?".into(),
            },
        ),
        None,
    ))
    .expect("ask")
    .state;
    (hive, asked)
}

#[test]
fn a_desk_brief_carries_assignment_waiting_and_the_parent_it_must_name() {
    let (_hive, state) = state_with_an_ask();
    let brief = EpisodeBrief::for_turn(
        &state,
        "engineering",
        "one",
        Channel::Desk,
        vec!["@two: hello".into()],
        Vec::new(),
    );
    assert_eq!(brief.assignment, Some(Sequence(0)));
    assert_eq!(brief.awaiting, ["two"]);
    assert_eq!(brief.queued, 0);
    assert_eq!(brief.parent(), None);
    let text = brief.render();
    assert!(text.contains("## New desk messages\n@two: hello"));
    assert!(text.contains("Your assignment was made at sequence 0."));
    assert!(text.contains("cannot complete until your conversation with @two concludes"));
    assert!(text.contains("\"chat\": \"engineering\" and \"parent\": null"));
    assert!(!text.contains("Conversations"), "none were given");
}

#[test]
fn a_seat_with_nothing_open_is_told_so_and_shown_nothing_new_plainly() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let state = driver.start(episode(&["one", "two"])).expect("state");
    let done = run(driver.apply_committed(
        &state,
        committed(
            "two",
            1,
            Utterance::CompleteEpisode {
                message: "done".into(),
            },
        ),
        None,
    ))
    .expect("complete")
    .state;
    let text = EpisodeBrief::for_turn(
        &done,
        "engineering",
        "two",
        Channel::Desk,
        Vec::new(),
        Vec::new(),
    )
    .render();
    assert!(text.contains("(nothing new)"));
    assert!(text.contains("You hold no open assignment."));
}

#[test]
fn conversations_are_split_into_concluded_and_in_progress_with_their_transcripts() {
    let (_hive, state) = state_with_an_ask();
    let brief = EpisodeBrief::for_turn(
        &state,
        "engineering",
        "one",
        Channel::Desk,
        Vec::new(),
        vec![
            ConversationView {
                root: Sequence(1),
                other: "two".into(),
                opened_it: true,
                transcript: vec!["@one: ?".into(), "@two: because".into()],
                concluded: true,
            },
            ConversationView {
                root: Sequence(4),
                other: "three".into(),
                opened_it: false,
                transcript: vec!["@three: and you?".into()],
                concluded: false,
            },
        ],
    );
    let text = brief.render();
    let concluded_at = text
        .find("## Conversations you had since you last spoke")
        .expect("concluded");
    let open_at = text
        .find("## Conversations still in progress")
        .expect("open");
    assert!(concluded_at < open_at, "concluded first, then open");
    assert!(text.contains("### With @two (thread 1)\n@one: ?\n@two: because"));
    assert!(text.contains("### With @three (thread 4) -- in progress\n@three: and you?"));
}

#[test]
fn a_thread_brief_names_its_root_as_the_parent_and_says_which_side_the_seat_is_on() {
    let (_hive, state) = state_with_an_ask();
    let asker = EpisodeBrief::for_turn(
        &state,
        "engineering",
        "one",
        Channel::Thread {
            root: Sequence(1),
            other: "two".into(),
            opened_it: true,
        },
        vec!["@two: because".into()],
        Vec::new(),
    );
    assert_eq!(asker.parent(), Some("1".into()));
    let text = asker.render();
    assert!(text.contains("## A private conversation with @two (thread 1)\n@two: because"));
    assert!(text.contains("You opened this conversation"));
    assert!(text.contains("\"parent\": \"1\""));
    assert!(text.contains("An `ask` or a `broadcast` made here goes to the desk"));
    let answerer = EpisodeBrief::for_turn(
        &state,
        "engineering",
        "two",
        Channel::Thread {
            root: Sequence(1),
            other: "one".into(),
            opened_it: false,
        },
        Vec::new(),
        Vec::new(),
    );
    assert!(answerer.render().contains("A peer asked you this."));
}

#[test]
fn the_standing_contract_is_the_specs_own_words_with_the_hosts_one_sentence() {
    let served = tool_specs().iter().filter(|spec| spec.name != "dm");
    let text = standing_contract(
        served,
        "engineering",
        "Use `mcp_call_tool` with `server: \"episode\"`.",
    );
    assert!(text.starts_with("Your work is recorded by calling a tool."));
    assert!(text.contains("Use `mcp_call_tool` with `server: \"episode\"`."));
    for name in ["post", "broadcast", "ask", "complete_episode", "read"] {
        assert!(text.contains(&format!("tool \"{name}\"")), "{name}");
    }
    assert!(!text.contains("tool \"dm\""));
    let ask = tool_specs()
        .iter()
        .find(|spec| spec.name == "ask")
        .expect("ask");
    assert!(
        text.contains(ask.description),
        "descriptions travel verbatim"
    );
    assert!(text.contains("arguments {\"to\": ..., \"message\": ...}"));
}
