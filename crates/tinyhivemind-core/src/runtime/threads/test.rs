//! Unit tests for the bounded, recency-ordered thread index.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::aside::Audience;
use crate::aside::Viewer;
use crate::runtime::{SessionAuthor, SessionFuture, SessionPage, SourceError};
use std::{collections::VecDeque, io, sync::Mutex};

#[derive(Debug)]
struct FakeLog {
    pages: Mutex<VecDeque<std::result::Result<SessionPage, SourceError>>>,
    calls: Mutex<usize>,
}

impl FakeLog {
    fn new(pages: Vec<SessionPage>) -> Self {
        Self {
            pages: Mutex::new(pages.into_iter().map(Ok).collect()),
            calls: Mutex::new(0),
        }
    }

    fn failing() -> Self {
        Self {
            pages: Mutex::new(VecDeque::from([Err(
                Box::new(io::Error::other("offline")) as SourceError
            )])),
            calls: Mutex::new(0),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.lock().expect("calls lock is not poisoned")
    }
}

impl SessionLog for FakeLog {
    fn read_before(&self, _before: Option<Sequence>, _limit: usize) -> SessionFuture<'_> {
        *self.calls.lock().expect("calls lock is not poisoned") += 1;
        Box::pin(async move {
            self.pages
                .lock()
                .expect("pages lock is not poisoned")
                .pop_front()
                .unwrap_or_else(|| Ok(SessionPage::default()))
        })
    }
}

fn conversation() -> Conversation {
    Conversation {
        desk_id: "engineering".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    }
}

fn message(sequence: u64, chat: Option<&str>, parent: Option<u64>, content: &str) -> LogMessage {
    LogMessage {
        sequence: Sequence(sequence),
        chat_id: chat.map(str::to_owned),
        parent: parent.map(Sequence),
        author: SessionAuthor::Operator,
        content: content.into(),
        audience: Audience::Desk,
    }
}

fn page(messages: Vec<LogMessage>, next: Option<u64>) -> SessionPage {
    SessionPage {
        messages,
        next_before: next.map(Sequence),
    }
}

#[test]
fn thread_line_pins_its_wire_shape() {
    let line = ThreadLine {
        root: Sequence(41),
        opening: "draft the launch email".into(),
        replies: 4,
        latest: Sequence(58),
        landed: Some("In review".into()),
    };
    let wire = serde_json::json!({
        "root": 41,
        "opening": "draft the launch email",
        "replies": 4,
        "latest": 58,
        "landed": "In review"
    });
    assert_eq!(serde_json::to_value(&line).expect("serializes"), wire);
    assert_eq!(
        serde_json::from_value::<ThreadLine>(wire).expect("deserializes"),
        line
    );
}

#[test]
fn fold_counts_replies_per_root_and_tracks_the_newest_activity() {
    let rows = vec![
        message(1, Some("engineering"), None, "draft the launch email"),
        message(2, Some("engineering"), None, "check the invoice"),
        message(3, Some("engineering"), Some(1), "on it"),
        message(4, Some("engineering"), Some(1), "here is a draft"),
    ];
    let index = fold_thread_index(&rows, &Viewer::Operator, THREAD_INDEX_LIMIT);
    assert_eq!(
        index
            .iter()
            .map(|line| (
                line.root.0,
                line.opening.as_str(),
                line.replies,
                line.latest.0
            ))
            .collect::<Vec<_>>(),
        vec![
            (1, "draft the launch email", 2, 4),
            (2, "check the invoice", 0, 2)
        ]
    );
    assert!(index.iter().all(|line| line.landed.is_none()));
}

#[test]
fn fold_orders_by_newest_activity_not_by_root() {
    let rows = vec![
        message(1, Some("engineering"), None, "older root"),
        message(2, Some("engineering"), None, "newer root"),
        message(3, Some("engineering"), Some(1), "revives the older thread"),
    ];
    assert_eq!(
        fold_thread_index(&rows, &Viewer::Operator, THREAD_INDEX_LIMIT)
            .iter()
            .map(|line| line.root.0)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn fold_keeps_only_the_most_recent_threads_up_to_the_limit() {
    let rows: Vec<LogMessage> = (1..=8)
        .map(|sequence| {
            message(
                sequence,
                Some("engineering"),
                None,
                &format!("thread {sequence}"),
            )
        })
        .collect();
    assert_eq!(
        fold_thread_index(&rows, &Viewer::Operator, 3)
            .iter()
            .map(|line| line.root.0)
            .collect::<Vec<_>>(),
        vec![8, 7, 6]
    );
    let found = fold_thread_index(&rows, &Viewer::Operator, 0);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn fold_ignores_blank_roots_blank_replies_and_replies_without_a_root() {
    let rows = vec![
        message(1, Some("engineering"), None, "  \n "),
        message(2, Some("engineering"), Some(1), "reply to a blank root"),
        message(3, Some("engineering"), Some(99), "reply to an unseen root"),
        message(4, Some("engineering"), None, "real root"),
        message(5, Some("engineering"), Some(4), "  "),
        message(6, Some("engineering"), Some(4), "counted"),
    ];
    let index = fold_thread_index(&rows, &Viewer::Operator, THREAD_INDEX_LIMIT);
    assert_eq!(
        index
            .iter()
            .map(|line| (line.root.0, line.replies, line.latest.0))
            .collect::<Vec<_>>(),
        vec![(4, 1, 6)]
    );
}

#[test]
fn opening_collapses_whitespace_and_truncates_on_a_character_boundary() {
    let rows = vec![
        message(
            1,
            Some("engineering"),
            None,
            "  draft\n  the   launch\temail  ",
        ),
        message(
            2,
            Some("engineering"),
            None,
            &"é".repeat(THREAD_OPENING_CHARS + 5),
        ),
        message(
            3,
            Some("engineering"),
            None,
            &"x".repeat(THREAD_OPENING_CHARS),
        ),
    ];
    let index = fold_thread_index(&rows, &Viewer::Operator, THREAD_INDEX_LIMIT);
    let opening = |root: u64| {
        index
            .iter()
            .find(|line| line.root.0 == root)
            .expect("indexed")
            .opening
            .clone()
    };
    assert_eq!(opening(1), "draft the launch email");
    assert_eq!(opening(2), format!("{}…", "é".repeat(THREAD_OPENING_CHARS)));
    assert_eq!(opening(3), "x".repeat(THREAD_OPENING_CHARS));
}

#[tokio::test]
async fn index_reads_pages_filters_the_desk_and_returns_newest_first() {
    let log = FakeLog::new(vec![
        page(
            vec![
                message(5, Some("engineering"), Some(1), "reply"),
                message(4, Some("other"), None, "elsewhere"),
            ],
            Some(4),
        ),
        page(
            vec![
                message(2, Some("Engineering"), None, "second thread"),
                message(1, Some("engineering"), None, "first thread"),
            ],
            None,
        ),
    ]);
    let index = read_thread_index(&log, &conversation(), &Viewer::Operator, THREAD_INDEX_LIMIT)
        .await
        .expect("indexes");
    assert_eq!(
        index
            .iter()
            .map(|line| (line.root.0, line.replies))
            .collect::<Vec<_>>(),
        vec![(1, 1), (2, 0)]
    );
    assert_eq!(log.call_count(), 2);
}

#[tokio::test]
async fn index_is_empty_and_unread_for_a_thread_or_a_zero_limit() {
    let log = FakeLog::new(vec![page(
        vec![message(1, Some("engineering"), None, "root")],
        None,
    )]);
    let mut inside = conversation();
    inside.thread_root = Some(Sequence(1));
    let found_found = read_thread_index(&log, &inside, &Viewer::Operator, THREAD_INDEX_LIMIT)
        .await
        .expect("indexes");
    assert!(found_found.is_empty(), "{found_found:?}");
    let found_found_found = read_thread_index(&log, &conversation(), &Viewer::Operator, 0)
        .await
        .expect("indexes");
    assert!(found_found_found.is_empty(), "{found_found_found:?}");
    assert_eq!(log.call_count(), 0);
}

#[tokio::test]
async fn index_stops_at_its_own_scan_bound_well_below_the_projection_limit() {
    const _: () = assert!(THREAD_INDEX_SCAN < crate::runtime::SCAN_LIMIT);
    // One full page exhausts the bound, so the root two rows further back is
    // never read and its thread is absent from the index.
    let rows = (0..THREAD_INDEX_SCAN as u64)
        .map(|offset| {
            message(
                THREAD_INDEX_SCAN as u64 - offset,
                Some("engineering"),
                Some(1),
                "reply",
            )
        })
        .collect();
    let log = FakeLog::new(vec![
        page(rows, Some(1)),
        page(
            vec![message(1, Some("engineering"), None, "unreachable root")],
            None,
        ),
    ]);
    let found_found_found_found =
        read_thread_index(&log, &conversation(), &Viewer::Operator, THREAD_INDEX_LIMIT)
            .await
            .expect("indexes");
    assert!(
        found_found_found_found.is_empty(),
        "{found_found_found_found:?}"
    );
    assert_eq!(log.call_count(), 1);
}

#[tokio::test]
async fn index_reports_read_and_validation_failures() {
    assert!(matches!(
        read_thread_index(
            &FakeLog::failing(),
            &conversation(),
            &Viewer::Operator,
            THREAD_INDEX_LIMIT
        )
        .await,
        Err(Error::Read { .. })
    ));

    let log = FakeLog::new(vec![page(Vec::new(), Some(4))]);
    assert!(matches!(
        read_thread_index(&log, &conversation(), &Viewer::Operator, THREAD_INDEX_LIMIT).await,
        Err(Error::EmptyPageCursor { .. })
    ));
}

// ---------------------------------------------------------------------------
// Private asides
// ---------------------------------------------------------------------------

fn aside(row: LogMessage, members: &[&str]) -> LogMessage {
    LogMessage {
        audience: Audience::Aside {
            members: members.iter().map(|member| (*member).to_owned()).collect(),
        },
        ..row
    }
}

fn reader(id: &str) -> Viewer {
    Viewer::Agent { id: id.into() }
}

#[test]
fn a_root_a_viewer_cannot_read_is_not_indexed() {
    // `ThreadLine::opening` is verbatim content, so a closed thread cannot
    // appear in a non-member's index at all. The exchange is not hidden by
    // that: its root still appears in the desk projection as a stub.
    let rows = [
        aside(
            message(1, Some("engineering"), None, "privately, about the keys"),
            &["auditor"],
        ),
        message(2, Some("engineering"), None, "in the open"),
    ];
    let index = fold_thread_index(&rows, &reader("archivist"), THREAD_INDEX_LIMIT);
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].root, Sequence(2));

    // Its members do see it.
    let member = fold_thread_index(&rows, &reader("auditor"), THREAD_INDEX_LIMIT);
    assert_eq!(member.len(), 2);
}

#[test]
fn a_reply_a_viewer_cannot_read_neither_counts_nor_advances_the_thread() {
    // A reply count and a `latest` sequence describe a message. One that moved
    // a thread up this index would tell a non-member both that something was
    // said and roughly when — a leak with no content in it.
    let rows = [
        message(1, Some("engineering"), None, "older thread"),
        message(2, Some("engineering"), Some(1), "a reply everyone sees"),
        message(3, Some("engineering"), None, "newer thread"),
        aside(
            message(4, Some("engineering"), Some(1), "privately"),
            &["auditor"],
        ),
    ];

    let outsider = fold_thread_index(&rows, &reader("archivist"), THREAD_INDEX_LIMIT);
    let older = outsider
        .iter()
        .find(|line| line.root == Sequence(1))
        .expect("the older thread");
    assert_eq!(older.replies, 1);
    assert_eq!(older.latest, Sequence(2));
    // The newer thread still sorts first, because nothing moved the older one.
    assert_eq!(outsider[0].root, Sequence(3));

    let member = fold_thread_index(&rows, &reader("auditor"), THREAD_INDEX_LIMIT);
    let older = member
        .iter()
        .find(|line| line.root == Sequence(1))
        .expect("the older thread");
    assert_eq!(older.replies, 2);
    assert_eq!(older.latest, Sequence(4));
    assert_eq!(member[0].root, Sequence(1));
}

#[test]
fn an_index_over_a_desk_with_no_aside_is_the_same_for_every_viewer() {
    let rows = [
        message(1, Some("engineering"), None, "one"),
        message(2, Some("engineering"), Some(1), "two"),
    ];
    let baseline = fold_thread_index(&rows, &Viewer::Operator, THREAD_INDEX_LIMIT);
    for viewer in [reader("anyone"), Viewer::Person { id: "ada".into() }] {
        assert_eq!(
            fold_thread_index(&rows, &viewer, THREAD_INDEX_LIMIT),
            baseline
        );
    }
}
