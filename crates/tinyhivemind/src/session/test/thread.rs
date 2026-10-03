//! Thread-scoped projection tests: walking a reply chain back to its root,
//! stopping at a blank root or the window, and propagating read failures.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, message, page, query};
use crate::Error;
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};

#[tokio::test]
async fn thread_keeps_root_and_direct_children_then_stops_at_root() {
    let log = FakeLog::new(vec![page(
        vec![
            message(8, Some("engineering"), Some(6), "nested elsewhere"),
            message(7, Some("engineering"), Some(5), "child"),
            message(6, Some("engineering"), Some(5), "child two"),
            message(5, Some("engineering"), None, "root"),
            message(4, Some("engineering"), None, "older"),
        ],
        None,
    )]);
    let mut query = query(8);
    query.conversation.thread_root = Some(Sequence(5));
    let history = project_session(&log, &query).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.sequence.0)
            .collect::<Vec<_>>(),
        vec![5, 6, 7]
    );
}

fn thread_query(window: usize, root: u64) -> SessionQuery {
    let mut query = query(window);
    query.conversation.thread_root = Some(Sequence(root));
    query
}

#[tokio::test]
async fn thread_projection_skips_a_blank_reply_and_stops_without_a_cursor() {
    let log = FakeLog::new(vec![page(
        vec![
            message(9, Some("engineering"), Some(5), "later"),
            message(8, Some("engineering"), Some(5), "   "),
            message(7, Some("engineering"), Some(5), "earlier"),
        ],
        None,
    )]);
    let history = project_session(&log, &thread_query(5, 5))
        .await
        .expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["earlier", "later"]
    );
    assert_eq!(log.call_count(), 1);
}

#[tokio::test]
async fn thread_projection_stops_reading_once_the_window_is_met() {
    let log = FakeLog::new(vec![
        page(
            vec![
                message(9, Some("engineering"), Some(5), "third"),
                message(8, Some("engineering"), Some(5), "second"),
                message(7, Some("engineering"), Some(5), "first"),
            ],
            Some(7),
        ),
        page(vec![message(5, Some("engineering"), None, "root")], None),
    ]);
    let history = project_session(&log, &thread_query(2, 5))
        .await
        .expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "third"]
    );
    assert_eq!(log.call_count(), 1);
}

#[tokio::test]
async fn thread_projection_follows_the_cursor_across_pages_to_its_root() {
    let log = FakeLog::new(vec![
        page(
            vec![message(9, Some("engineering"), Some(5), "reply")],
            Some(9),
        ),
        page(
            vec![
                message(7, Some("other"), None, "elsewhere"),
                message(5, Some("engineering"), None, "root"),
            ],
            Some(5),
        ),
    ]);
    let history = project_session(&log, &thread_query(5, 5))
        .await
        .expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.content.as_str())
            .collect::<Vec<_>>(),
        vec!["root", "reply"]
    );
    assert_eq!(log.call_count(), 2);
}

#[tokio::test]
async fn thread_scan_cap_is_a_successful_partial_projection() {
    let mut pages = Vec::new();
    for page_index in 0_u64..4 {
        let high = 3_000 - page_index * PAGE_SIZE as u64;
        let messages = (0..PAGE_SIZE as u64)
            .map(|offset| message(high - offset, Some("other"), None, "ignored"))
            .collect::<Vec<_>>();
        pages.push(page(messages, Some(high - (PAGE_SIZE as u64 - 1))));
    }
    let log = FakeLog::new(pages);
    let found = project_session(&log, &thread_query(1, 5))
        .await
        .expect("scan cap succeeds");
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(log.call_count(), 4);
}

#[tokio::test]
async fn thread_projection_reports_read_and_validation_failures() {
    let error = project_session(&FakeLog::failing(), &thread_query(2, 5))
        .await
        .expect_err("read fails");
    assert!(matches!(error, Error::Read { .. }));

    let oversized = (0..=PAGE_SIZE as u64)
        .map(|offset| message(2_000 - offset, Some("other"), None, "row"))
        .collect();
    let log = FakeLog::new(vec![page(oversized, None)]);
    assert!(matches!(
        project_session(&log, &thread_query(1, 5)).await,
        Err(Error::PageTooLarge { .. })
    ));
}

#[tokio::test]
async fn whitespace_thread_root_stops_before_another_read() {
    let log = FakeLog {
        pages: Mutex::new(VecDeque::from([
            Ok(page(
                vec![message(5, Some("engineering"), None, " \n\t ")],
                Some(5),
            )),
            Err(Box::new(io::Error::other("must not read past root")) as SourceError),
        ])),
        calls: Arc::default(),
    };
    let mut query = query(8);
    query.conversation.thread_root = Some(Sequence(5));
    let found_found = project_session(&log, &query)
        .await
        .expect("root terminates the walk");
    assert!(found_found.is_empty(), "{found_found:?}");
    assert_eq!(log.call_count(), 1);
}

#[tokio::test]
async fn skips_trim_empty_content_but_preserves_other_bytes_and_author() {
    let author = SessionAuthor::Person {
        id: "p1".into(),
        label: "Pat".into(),
    };
    let mut kept = message(4, Some("engineering"), None, "  keep me  \n");
    kept.author = author.clone();
    let log = FakeLog::new(vec![page(
        vec![kept, message(3, Some("engineering"), None, " \n\t ")],
        None,
    )]);
    let history = project_session(&log, &query(3)).await.expect("projects");
    assert_eq!(history[0].content, "  keep me  \n");
    assert_eq!(history[0].author, author);
}

#[tokio::test]
async fn propagates_source_errors_with_their_source() {
    let error = project_session(&FakeLog::failing(), &query(2))
        .await
        .expect_err("read fails");
    assert!(matches!(error, Error::Read { .. }));
    assert!(std::error::Error::source(&error).is_some());
}
