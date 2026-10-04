//! The board through core: commit, mentions, pins, digest and the briefing.

use tinyhivemind_core::runtime::speech::Utterance;

use super::*;

fn post(text: &str) -> Utterance {
    Utterance::Post {
        message: text.into(),
    }
}

fn board() -> Board {
    Board::new(&["lead", "implementer", "tester"], 6)
}

#[test]
fn commit_resolves_mentions_to_seats() {
    let board = board();
    let done = board
        .commit("lead", &post("@implementer please fix, then @tester"))
        .expect("commit");
    assert_eq!(done.addressed, ["implementer", "tester"]);
    assert!(!done.completes);
    assert_eq!(board.len(), 1);
}

#[test]
fn an_ask_addresses_its_target_and_complete_episode_completes() {
    let board = board();
    let ask = board
        .commit(
            "lead",
            &Utterance::Ask {
                to: vec!["tester".into()],
                message: "does it pass".into(),
            },
        )
        .expect("ask");
    assert_eq!(ask.addressed, ["tester"]);
    let done = board
        .commit("lead", &Utterance::CompleteEpisode { message: "done".into() })
        .expect("complete");
    assert!(done.completes);
}

#[test]
fn an_unknown_speaker_is_an_error_not_a_row() {
    let board = board();
    assert!(board.commit("ghost", &post("hi")).is_err() || board.len() == 1);
}

#[test]
fn briefing_shows_pins_then_recent_rows() {
    let board = board();
    board.commit("lead", &post("plan: edit parser.py\n!pin #plan")).expect("c");
    board.commit("implementer", &post("patched parser.py")).expect("c");
    let text = board.briefing("tester");
    let pinned = text.find("## Pinned").expect("pins");
    let recent = text.find("## Recent").expect("recent");
    assert!(pinned < recent);
    assert!(text.contains("#plan"));
    assert!(text.contains("@implementer: patched parser.py"));
}

#[test]
fn old_rows_fold_into_a_bounded_digest_and_leave_the_tail() {
    let board = board();
    for n in 0..40 {
        board.commit("lead", &post(&format!("note number {n} about the parser"))).expect("c");
    }
    assert_eq!(board.maintain(), Some(1));
    let text = board.briefing("tester");
    assert!(text.contains("## Earlier on the desk (digest)"));
    assert!(text.contains("note number 39"));
    let live = text.split("## Recent desk messages").nth(1).expect("tail");
    assert!(!live.contains("note number 0 "), "folded rows leave the tail");
    assert!(text.len() < 6000, "briefing stays bounded, was {}", text.len());
}

#[test]
fn read_returns_a_bounded_window() {
    let board = board();
    for n in 0..10 {
        board.commit("lead", &post(&format!("m{n}"))).expect("c");
    }
    let rows = board.read("tester", 3);
    assert_eq!(rows.lines().count(), 3);
    assert!(rows.ends_with("m9"));
    assert_eq!(Board::new(&["a"], 4).read("a", 5), "(no messages)");
}

#[test]
fn extractive_fold_respects_the_budget() {
    use tinyhivemind_core::runtime::{Conversation, Sequence, SessionMessage};
    use tinyhivemind_core::runtime::digest::DigestRequest;
    let messages = (1..=50)
        .map(|n| SessionMessage {
            sequence: Sequence(n),
            author: crate::agent("lead"),
            content: format!("row {n} {}", "x".repeat(150)),
            audience: tinyhivemind_core::aside::Audience::Desk,
            elided: None,
        })
        .collect();
    let request = DigestRequest {
        conversation: Conversation { desk_id: "swe".into(), desk_name: "s".into(), thread_root: None },
        prior: None,
        messages,
        through: Sequence(50),
        budget_chars: 500,
        pinned: vec![Sequence(50)],
    };
    let text = super::digester::fold_for_test(&request);
    assert!(text.len() <= 500);
    assert!(text.contains("* ^50"));
}
