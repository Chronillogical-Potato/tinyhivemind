//! Unit tests for the pin grammar and the board it folds to.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::{SessionFuture, SessionPage, SourceError};
use std::{collections::VecDeque, io, sync::Mutex};
use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::aside::Viewer;

#[derive(Debug)]
struct FakeLog {
    pages: Mutex<VecDeque<std::result::Result<SessionPage, SourceError>>>,
    calls: Mutex<Vec<Option<Sequence>>>,
}

impl FakeLog {
    fn new(pages: Vec<SessionPage>) -> Self {
        Self {
            pages: Mutex::new(pages.into_iter().map(Ok).collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            pages: Mutex::new(VecDeque::from([Err(
                Box::new(io::Error::other("offline")) as SourceError
            )])),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().expect("calls lock is not poisoned").len()
    }

    fn first_call_before(&self) -> Option<Sequence> {
        self.calls.lock().expect("calls lock is not poisoned")[0]
    }
}

impl SessionLog for FakeLog {
    fn read_before(&self, before: Option<Sequence>, _limit: usize) -> SessionFuture<'_> {
        self.calls
            .lock()
            .expect("calls lock is not poisoned")
            .push(before);
        Box::pin(async move {
            self.pages
                .lock()
                .expect("pages lock is not poisoned")
                .pop_front()
                .unwrap_or_else(|| Ok(SessionPage::default()))
        })
    }
}

fn agent(id: &str) -> SessionAuthor {
    SessionAuthor::Agent {
        id: id.to_owned(),
        label: id.to_owned(),
    }
}

fn row(sequence: u64, chat: Option<&str>, parent: Option<u64>, content: &str) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: chat.map(ToOwned::to_owned),
        parent: parent.map(Sequence),
        author: agent("alice"),
        content: content.to_owned(),
        audience: Audience::Desk,
    }
}

fn conversation(thread_root: Option<u64>) -> Conversation {
    Conversation {
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        thread_root: thread_root.map(Sequence),
    }
}

#[test]
fn pins_the_carrying_message_when_no_target_is_named() {
    let directives = read_directives("!pin", &agent("alice"), Sequence(4));
    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].target, Sequence(4));
    assert_eq!(directives[0].action, PinAction::Pin);
}

#[test]
fn reads_a_target_a_label_and_a_note() {
    let directives = read_directives(
        "!pin ^2 #limits why it matters",
        &agent("alice"),
        Sequence(5),
    );
    let directive = &directives[0];
    assert_eq!(directive.target, Sequence(2));
    assert_eq!(directive.label.as_deref(), Some("limits"));
    assert_eq!(directive.note.as_deref(), Some("why it matters"));
}

#[test]
fn ignores_a_marker_inside_a_fenced_block_and_one_that_is_not_line_leading() {
    let body = "look at `!pin` here\n```\n!pin ^2\n```\n  !pin ^3";
    let directives = read_directives(body, &agent("alice"), Sequence(9));
    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].target, Sequence(3));
}

#[test]
fn ignores_a_marker_inside_a_multi_line_code_span() {
    // A code span that opens on one line and closes on a later one quotes
    // every line between them, so the directive inside it is documentation
    // and only the one below the span pins anything.
    let body = "run `\n!pin ^1\n` carefully\n!pin ^2";
    let directives = read_directives(body, &agent("alice"), Sequence(9));
    assert_eq!(directives.len(), 1);
    assert_eq!(directives[0].target, Sequence(2));
}

#[test]
fn requires_a_closing_fence_at_least_as_long_as_the_opener() {
    // The outer block opens with four backticks. A three-backtick line
    // inside it is Markdown content — an example fence being quoted — not a
    // close, so the `!unpin` on the line after it must stay inert.
    let body = "````\nexample:\n```\n!unpin ^1\n```\nstill inside\n````";
    let found = read_directives(body, &agent("alice"), Sequence(9));
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn ignores_an_unknown_marker_and_a_body_with_no_marker_at_all() {
    let found = read_directives("!propose #x", &agent("alice"), Sequence(1));
    assert!(found.is_empty(), "{found:?}");
    let found = read_directives("ordinary conversation", &agent("alice"), Sequence(1));
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn refuses_an_unpin_with_no_target() {
    let found = read_directives("!unpin", &agent("alice"), Sequence(3));
    assert!(found.is_empty(), "{found:?}");
    let directives = read_directives("!unpin ^2", &agent("alice"), Sequence(3));
    assert_eq!(directives[0].action, PinAction::Unpin);
    assert_eq!(directives[0].target, Sequence(2));
}

#[test]
fn caps_the_markers_read_from_one_body() {
    let body = "!pin ^1\n".repeat(PIN_MARKER_CAP + 4);
    assert_eq!(
        read_directives(&body, &agent("alice"), Sequence(9)).len(),
        PIN_MARKER_CAP
    );
}

#[test]
fn folds_a_pin_with_its_excerpt() {
    let rows = [
        row(1, None, None, "The rate limiter resets at midnight UTC."),
        row(2, None, None, "!pin ^1 #limits worth keeping"),
    ];
    let board = fold_pins(&rows, &Viewer::Operator, PIN_LIMIT);
    assert_eq!(board.len(), 1);
    assert_eq!(board[0].sequence, Sequence(1));
    assert_eq!(board[0].pinned_at, Sequence(2));
    assert_eq!(
        board[0].excerpt.as_deref(),
        Some("The rate limiter resets at midnight UTC.")
    );
}

#[test]
fn leaves_no_excerpt_when_the_pinned_row_is_outside_the_slice() {
    let rows = [row(9, None, None, "!pin ^1")];
    let board = fold_pins(&rows, &Viewer::Operator, PIN_LIMIT);
    assert_eq!(board[0].sequence, Sequence(1));
    assert!(board[0].excerpt.is_none());
}

#[test]
fn leaves_no_excerpt_for_a_blank_pinned_row() {
    let rows = [row(1, None, None, "   "), row(2, None, None, "!pin ^1")];
    assert!(
        fold_pins(&rows, &Viewer::Operator, PIN_LIMIT)[0]
            .excerpt
            .is_none()
    );
}

#[test]
fn a_later_pin_updates_the_one_already_on_the_board() {
    let rows = [
        row(1, None, None, "a decision"),
        row(2, None, None, "!pin ^1 #early first reason"),
        row(3, None, None, "!pin ^1 #late better reason"),
    ];
    let board = fold_pins(&rows, &Viewer::Operator, PIN_LIMIT);
    assert_eq!(board.len(), 1);
    assert_eq!(board[0].label.as_deref(), Some("late"));
    assert_eq!(board[0].note.as_deref(), Some("better reason"));
    assert_eq!(board[0].pinned_at, Sequence(3));
}

#[test]
fn an_unpin_takes_a_message_back_off() {
    let rows = [
        row(1, None, None, "a decision"),
        row(2, None, None, "!pin ^1"),
        row(3, None, None, "!unpin ^1"),
    ];
    let found = fold_pins(&rows, &Viewer::Operator, PIN_LIMIT);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn orders_the_board_most_recently_pinned_first_and_drops_the_oldest_over_the_limit() {
    let rows: Vec<LogMessage> = (1..=5)
        .map(|sequence| row(sequence, None, None, &format!("!pin ^{sequence}")))
        .collect();
    let board = fold_pins(&rows, &Viewer::Operator, 3);
    let pinned: Vec<Sequence> = board.iter().map(|pin| pin.sequence).collect();
    assert_eq!(pinned, vec![Sequence(5), Sequence(4), Sequence(3)]);
}

#[test]
fn folds_nothing_at_a_zero_limit() {
    let rows = [row(2, None, None, "!pin ^1")];
    let found = fold_pins(&rows, &Viewer::Operator, 0);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn truncates_a_long_excerpt_on_a_character_boundary() {
    let long = "é".repeat(PIN_EXCERPT_CHARS + 40);
    let rows = [row(1, None, None, &long), row(2, None, None, "!pin ^1")];
    let excerpt = fold_pins(&rows, &Viewer::Operator, PIN_LIMIT)[0]
        .excerpt
        .clone()
        .expect("excerpt");
    assert_eq!(excerpt.chars().count(), PIN_EXCERPT_CHARS + 1);
    assert!(excerpt.ends_with('…'));
}

#[tokio::test]
async fn reads_a_desk_board_including_thread_interiors() {
    let log = FakeLog::new(vec![SessionPage {
        messages: vec![
            row(4, Some("engineering"), Some(2), "!pin ^3"),
            row(3, Some("engineering"), Some(2), "buried insight"),
            row(2, Some("engineering"), None, "thread opening"),
            row(1, Some("support"), None, "!pin"),
        ],
        next_before: None,
    }]);
    let board = read_pinboard(
        &log,
        &conversation(None),
        &Viewer::Operator,
        PIN_LIMIT,
        None,
    )
    .await
    .expect("reads");
    assert_eq!(board.len(), 1);
    assert_eq!(board[0].sequence, Sequence(3));
    assert_eq!(board[0].excerpt.as_deref(), Some("buried insight"));
}

#[tokio::test]
async fn reads_a_thread_board_from_that_thread_alone() {
    let log = FakeLog::new(vec![SessionPage {
        messages: vec![
            row(5, Some("engineering"), None, "!pin ^4"),
            row(4, Some("engineering"), None, "another root"),
            row(3, Some("engineering"), Some(2), "!pin"),
            row(2, Some("engineering"), None, "thread opening"),
        ],
        next_before: None,
    }]);
    let board = read_pinboard(
        &log,
        &conversation(Some(2)),
        &Viewer::Operator,
        PIN_LIMIT,
        None,
    )
    .await
    .expect("reads");
    assert_eq!(board.len(), 1);
    assert_eq!(board[0].sequence, Sequence(3));
}

#[tokio::test]
async fn honors_the_query_bound_when_reading_the_board() {
    let log = FakeLog::new(vec![SessionPage {
        messages: vec![row(2, Some("engineering"), None, "!pin")],
        next_before: None,
    }]);
    let bound = Sequence(5);
    read_pinboard(
        &log,
        &conversation(None),
        &Viewer::Operator,
        PIN_LIMIT,
        Some(bound),
    )
    .await
    .expect("reads");
    assert_eq!(log.first_call_before(), Some(bound));
}

#[test]
fn preserves_directive_order_within_one_message() {
    // Two markers in the same message share a `pinned_at` sequence, so the
    // fold has to track reading order itself: `^5` was written after `^3`
    // and must survive truncation ahead of it.
    let rows = [
        row(5, None, None, "fifth"),
        row(3, None, None, "third"),
        row(6, None, None, "!pin ^3\n!pin ^5"),
    ];
    let pins = fold_pins(&rows, &Viewer::Operator, 1);
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].sequence, Sequence(5));
}

#[tokio::test]
async fn reads_nothing_at_a_zero_limit_and_reports_a_read_failure() {
    let log = FakeLog::new(vec![SessionPage::default()]);
    let found_found = read_pinboard(&log, &conversation(None), &Viewer::Operator, 0, None)
        .await
        .expect("reads");
    assert!(found_found.is_empty(), "{found_found:?}");
    assert_eq!(log.call_count(), 0);

    let failing = FakeLog::failing();
    let error = read_pinboard(
        &failing,
        &conversation(None),
        &Viewer::Operator,
        PIN_LIMIT,
        None,
    )
    .await
    .expect_err("read fails");
    assert!(matches!(error, crate::Error::Read { .. }));
}

#[test]
fn renders_a_board_as_one_briefing_note() {
    let rows = [
        row(1, None, None, "midnight UTC"),
        row(2, None, None, "!pin ^1 #limits worth keeping"),
        row(3, None, None, "!pin ^9"),
    ];
    let note = pin_note(&fold_pins(&rows, &Viewer::Operator, PIN_LIMIT)).expect("note");
    assert_eq!(note.heading, "Pinned in this conversation");
    assert_eq!(note.lines[0], "[9]");
    assert_eq!(
        note.lines[1],
        "[1] #limits \"midnight UTC\" — worth keeping"
    );
}

#[test]
fn renders_no_note_for_an_empty_board() {
    assert!(pin_note(&[]).is_none());
}

#[test]
fn pins_the_wire_form_of_a_pin_and_a_directive() {
    let pin = Pin {
        sequence: Sequence(1),
        pinned_at: Sequence(2),
        pinned_by: agent("alice"),
        label: Some("limits".into()),
        note: Some("worth keeping".into()),
        excerpt: Some("midnight UTC".into()),
    };
    let json = serde_json::json!({
        "sequence": 1,
        "pinned_at": 2,
        "pinned_by": { "type": "agent", "id": "alice", "label": "alice" },
        "label": "limits",
        "note": "worth keeping",
        "excerpt": "midnight UTC"
    });
    assert_eq!(serde_json::to_value(&pin).expect("serializes"), json);
    assert_eq!(
        serde_json::from_value::<Pin>(json).expect("deserializes"),
        pin
    );

    let directive = &read_directives("!pin ^1", &agent("alice"), Sequence(2))[0];
    assert_eq!(
        serde_json::to_value(directive).expect("serializes"),
        serde_json::json!({
            "sequence": 2,
            "target": 1,
            "author": { "type": "agent", "id": "alice", "label": "alice" },
            "action": "pin",
            "label": null,
            "note": null
        })
    );
}

// ---------------------------------------------------------------------------
// Private asides
// ---------------------------------------------------------------------------

fn aside_row(sequence: u64, id: &str, members: &[&str], content: &str) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: Some("engineering".into()),
        parent: None,
        author: agent(id),
        content: content.into(),
        audience: Audience::Aside {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
        },
    }
}

fn desk_row(sequence: u64, id: &str, content: &str) -> LogMessage {
    LogMessage {
        author: agent(id),
        ..row(sequence, Some("engineering"), None, content)
    }
}

fn viewer(id: &str) -> Viewer {
    Viewer::Agent { id: id.into() }
}

#[test]
fn a_non_member_never_receives_an_excerpt_of_an_aside() {
    // The shortest path from a private message to somebody else's system
    // prompt: pin it, and let `pin_note` render 120 verbatim characters.
    let rows = [
        aside_row(
            1,
            "planner",
            &["auditor"],
            "The credentials rotate on Friday.",
        ),
        desk_row(2, "auditor", "!pin ^1 #creds"),
    ];

    let board = fold_pins(&rows, &viewer("archivist"), PIN_LIMIT);
    assert!(
        board.is_empty(),
        "a pin the reader cannot open is not a working set entry"
    );

    let note = pin_note(&board);
    assert!(note.is_none());
}

#[test]
fn a_member_pins_inside_its_own_aside_and_reads_the_excerpt() {
    // The other half of the same rule, and the one that makes the mechanism
    // usable: an agent that cannot re-find what it said privately once the
    // window has moved past it is worse off than if it had never said it.
    let rows = [
        aside_row(
            1,
            "planner",
            &["auditor"],
            "The credentials rotate on Friday.",
        ),
        aside_row(2, "auditor", &["planner"], "!pin ^1 #creds"),
    ];
    for id in ["planner", "auditor"] {
        let board = fold_pins(&rows, &viewer(id), PIN_LIMIT);
        assert_eq!(board.len(), 1, "{id} is in this aside");
        assert_eq!(board[0].sequence, Sequence(1));
        assert_eq!(
            board[0].excerpt.as_deref(),
            Some("The credentials rotate on Friday."),
        );
    }
}

#[test]
fn a_marker_in_an_unreadable_row_never_touches_the_board() {
    // The directive was never visible to this viewer, so it never happened to
    // its board. Reading it anyway would let an aside silently rearrange what
    // a non-member is told to keep.
    let rows = [
        desk_row(1, "planner", "The rate limiter resets at midnight."),
        aside_row(2, "auditor", &["planner"], "!unpin ^1"),
        desk_row(3, "archivist", "!pin ^1 #limits"),
    ];
    let outsider = fold_pins(&rows, &viewer("scribe"), PIN_LIMIT);
    assert_eq!(outsider.len(), 1);
    assert_eq!(outsider[0].sequence, Sequence(1));

    // A member of the aside saw the unpin, so for it the pin came off and the
    // later `!pin` put it back.
    let member = fold_pins(&rows, &viewer("planner"), PIN_LIMIT);
    assert_eq!(member.len(), 1);
}

#[test]
fn a_desk_pin_is_unaffected_by_an_aside_beside_it() {
    let rows = [
        desk_row(1, "planner", "The rate limiter resets at midnight."),
        aside_row(2, "planner", &["auditor"], "and privately, something else"),
        desk_row(3, "auditor", "!pin ^1 #limits"),
    ];
    let board = fold_pins(&rows, &viewer("archivist"), PIN_LIMIT);
    assert_eq!(board.len(), 1);
    assert_eq!(
        board[0].excerpt.as_deref(),
        Some("The rate limiter resets at midnight."),
    );
}
