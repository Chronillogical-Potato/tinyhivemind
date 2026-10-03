//! Page-validation tests: [`validate_page`] and the scan-cap behavior of
//! [`project_session`] when a host misbehaves or a projection exhausts
//! [`SCAN_LIMIT`] before its window is met.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, message, page, query};
use crate::runtime::Error;

#[tokio::test]
async fn zero_window_performs_no_read() {
    let log = FakeLog::new(Vec::new());
    let found = project_session(&log, &query(0)).await.expect("projects");
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(log.call_count(), 0);
}

#[tokio::test]
async fn initial_bound_is_exclusive_and_excludes_current_message() {
    let log = FakeLog::new(vec![page(
        vec![message(9, Some("engineering"), None, "old")],
        None,
    )]);
    let mut query = query(5);
    query.before = Some(Sequence(10));
    let history = project_session(&log, &query).await.expect("projects");
    assert_eq!(history[0].sequence, Sequence(9));
    assert_eq!(
        log.calls.lock().expect("calls lock")[0].0,
        Some(Sequence(10))
    );
}

#[tokio::test]
async fn reads_multiple_pages_and_returns_chronological_window() {
    let log = FakeLog::new(vec![
        page(
            vec![
                message(6, Some("engineering"), None, "six"),
                message(5, Some("other"), None, "other"),
                message(4, Some("engineering"), None, "four"),
            ],
            Some(4),
        ),
        page(
            vec![
                message(3, Some("engineering"), None, "three"),
                message(2, Some("engineering"), None, "two"),
            ],
            None,
        ),
    ]);
    let history = project_session(&log, &query(4)).await.expect("projects");
    assert_eq!(
        history
            .iter()
            .map(|item| item.sequence.0)
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 6]
    );
    assert_eq!(log.call_count(), 2);
}

#[tokio::test]
async fn rejects_a_row_at_or_above_the_exclusive_bound() {
    let log = FakeLog::new(vec![page(
        vec![message(10, Some("engineering"), None, "bad")],
        None,
    )]);
    let mut query = query(2);
    query.before = Some(Sequence(10));
    assert!(matches!(
        project_session(&log, &query).await,
        Err(Error::PageOutOfRange { .. })
    ));
}

#[tokio::test]
async fn rejects_rows_that_are_not_strictly_descending() {
    let log = FakeLog::new(vec![page(
        vec![message(3, None, None, "a"), message(4, None, None, "b")],
        None,
    )]);
    assert!(matches!(
        project_session(&log, &query(2)).await,
        Err(Error::PageNotDescending { .. })
    ));
}

#[tokio::test]
async fn rejects_duplicate_sequences() {
    let log = FakeLog::new(vec![page(
        vec![message(3, None, None, "a"), message(3, None, None, "b")],
        None,
    )]);
    assert!(matches!(
        project_session(&log, &query(2)).await,
        Err(Error::DuplicateSequence { .. })
    ));
}

#[tokio::test]
async fn rejects_an_empty_page_with_a_cursor() {
    let log = FakeLog::new(vec![page(Vec::new(), Some(4))]);
    assert!(matches!(
        project_session(&log, &query(2)).await,
        Err(Error::EmptyPageCursor { .. })
    ));
}

#[tokio::test]
async fn rejects_a_cursor_that_does_not_advance() {
    let log = FakeLog::new(vec![page(vec![message(9, None, None, "a")], Some(10))]);
    let mut query = query(2);
    query.before = Some(Sequence(10));
    assert!(matches!(
        project_session(&log, &query).await,
        Err(Error::CursorDidNotAdvance { .. })
    ));
}

#[tokio::test]
async fn accepts_a_cursor_older_than_the_oldest_row() {
    let log = FakeLog::new(vec![page(
        vec![message(9, None, None, "a"), message(8, None, None, "b")],
        Some(7),
    )]);
    assert!(project_session(&log, &query(2)).await.is_ok());
}

#[tokio::test]
async fn rejects_a_cursor_newer_than_the_oldest_row() {
    let log = FakeLog::new(vec![page(
        vec![message(9, None, None, "a"), message(8, None, None, "b")],
        Some(9),
    )]);
    assert!(matches!(
        project_session(&log, &query(2)).await,
        Err(Error::CursorAfterOldest { .. })
    ));
}

#[tokio::test]
async fn rejects_a_page_larger_than_the_requested_limit() {
    let messages = (0..=PAGE_SIZE as u64)
        .map(|offset| message(2_000 - offset, Some("other"), None, "row"))
        .collect();
    let log = FakeLog::new(vec![page(messages, None)]);
    assert!(matches!(
        project_session(&log, &query(1)).await,
        Err(Error::PageTooLarge {
            requested: PAGE_SIZE,
            actual
        }) if actual == PAGE_SIZE + 1
    ));
}

#[tokio::test]
async fn scan_cap_is_a_successful_partial_projection() {
    let mut pages = Vec::new();
    for page_index in 0_u64..4 {
        let high = 3_000 - page_index * PAGE_SIZE as u64;
        let messages = (0..PAGE_SIZE as u64)
            .map(|offset| message(high - offset, Some("other"), None, "ignored"))
            .collect::<Vec<_>>();
        pages.push(page(messages, Some(high - (PAGE_SIZE as u64 - 1))));
    }
    let log = FakeLog::new(pages);
    let found_found = project_session(&log, &query(1))
        .await
        .expect("scan cap succeeds");
    assert!(found_found.is_empty(), "{found_found:?}");
    assert_eq!(log.call_count(), 4);
}
