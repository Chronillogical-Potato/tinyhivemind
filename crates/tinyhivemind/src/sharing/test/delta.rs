//! `prepare_delta` walk tests: attribution and chronological order,
//! watermark/present-set filtering, desk and thread scoping across General
//! aliases, paging mechanics reusing P4 page validation, and the
//! retry/compare-and-swap contract a host relies on.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, conversation, delta, page, plan, raw, state};
use crate::{Error, SessionAuthor};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use tinyhivemind_core::aside::Viewer;

#[tokio::test]
async fn interleaved_local_and_peer_rows_are_attributed_and_chronological() {
    let log = FakeLog::new(vec![page(
        vec![
            raw(20, Some("engineering"), None, "local"),
            raw(19, Some("Engineering"), None, "peer"),
            raw(18, Some("engineering"), None, "watermark"),
        ],
        None,
    )]);
    let result = delta(plan(&log, &state(18), 21).await.expect("plans"));
    assert_eq!(
        result
            .messages
            .iter()
            .map(|row| row.sequence)
            .collect::<Vec<_>>(),
        vec![Sequence(19), Sequence(20)]
    );
    assert!(matches!(
        result.messages[0].author,
        SessionAuthor::Agent { ref id, .. } if id == "agent-19"
    ));
    assert_eq!(result.next_state.watermark, Sequence(21));
}

#[tokio::test]
async fn exclusive_before_and_watermark_rows_are_not_emitted() {
    let log = FakeLog::new(vec![page(
        vec![raw(10, Some("engineering"), None, "watermark")],
        None,
    )]);
    let result = delta(plan(&log, &state(10), 11).await.expect("plans"));
    assert!(result.messages.is_empty(), "{:?}", result.messages);
}

#[tokio::test]
async fn already_present_rows_are_omitted_and_later_concurrent_rows_are_retained() {
    let mut state = state(18);
    note_present(&mut state, Sequence(20)).expect("present");
    note_present(&mut state, Sequence(25)).expect("future present");
    let log = FakeLog::new(vec![page(
        vec![
            raw(20, Some("engineering"), None, "already"),
            raw(19, Some("engineering"), None, "new"),
            raw(18, Some("engineering"), None, "watermark"),
        ],
        None,
    )]);
    let result = delta(plan(&log, &state, 21).await.expect("plans"));
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].sequence, Sequence(19));
    assert_eq!(
        result.next_state.present_above_watermark,
        BTreeSet::from([Sequence(25)])
    );
}

#[tokio::test]
async fn unrelated_desks_and_blank_rows_count_but_are_filtered() {
    let log = FakeLog::new(vec![page(
        vec![
            raw(13, Some("sales"), None, "other"),
            raw(12, Some("engineering"), None, "  "),
            raw(11, Some("engineering"), None, "kept"),
            raw(10, Some("engineering"), None, "watermark"),
        ],
        None,
    )]);
    let result = delta(plan(&log, &state(10), 14).await.expect("plans"));
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].content, "kept");
}

#[tokio::test]
async fn general_aliases_share_one_delta() {
    let desired = conversation("general", "General", None);
    let current = conversation("main", "Anything", None);
    let state = initialized_state(conversation("", "GENERAL", None), Sequence(10));
    let log = FakeLog::new(vec![page(
        vec![
            raw(11, None, None, "general"),
            raw(10, Some("general"), None, "watermark"),
        ],
        None,
    )]);
    let result = prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &desired,
            current_conversation: &current,
            state: &state,
            before: Sequence(12),
            viewer: &Viewer::Operator,
        },
    )
    .await
    .expect("plans");
    assert_eq!(delta(result).messages.len(), 1);
}

#[tokio::test]
async fn channels_and_exact_threads_never_mix() {
    let thread = conversation("engineering", "Engineering", Some(10));
    let state = initialized_state(thread.clone(), Sequence(10));
    let log = FakeLog::new(vec![page(
        vec![
            raw(14, Some("engineering"), Some(11), "other thread"),
            raw(13, Some("engineering"), None, "channel"),
            raw(12, Some("engineering"), Some(10), "our thread"),
            raw(10, Some("engineering"), None, "root"),
        ],
        None,
    )]);
    let result = prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &thread,
            current_conversation: &thread,
            state: &state,
            before: Sequence(15),
            viewer: &Viewer::Operator,
        },
    )
    .await
    .expect("plans");
    let result = delta(result);
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].sequence, Sequence(12));
}

#[tokio::test]
async fn sparse_pages_continue_until_any_raw_row_crosses_watermark() {
    let log = FakeLog::new(vec![
        page(vec![raw(20, Some("sales"), None, "other")], Some(20)),
        page(vec![raw(15, Some("sales"), None, "other")], Some(15)),
        page(
            vec![
                raw(11, Some("engineering"), None, "new"),
                raw(10, Some("sales"), None, "cross"),
            ],
            None,
        ),
    ]);
    let result = delta(plan(&log, &state(10), 21).await.expect("plans"));
    assert_eq!(result.messages.len(), 1);
    assert_eq!(log.calls(), 3);
}

#[tokio::test]
async fn read_errors_propagate_without_mutating_input() {
    let log = FakeLog::failing();
    let state = state(10);
    let before = state.clone();
    assert!(matches!(
        plan(&log, &state, 12).await,
        Err(Error::Read { .. })
    ));
    assert_eq!(state, before);
}

#[tokio::test]
async fn malformed_pages_reuse_p4_validation() {
    let log = FakeLog::new(vec![page(
        vec![
            raw(11, Some("engineering"), None, "a"),
            raw(12, Some("engineering"), None, "b"),
        ],
        None,
    )]);
    assert!(matches!(
        plan(&log, &state(10), 13).await,
        Err(Error::PageNotDescending { .. })
    ));
}

#[tokio::test]
async fn every_p4_page_contract_failure_propagates_from_delta_planning() {
    let stored = state(1);

    let out_of_range = FakeLog::new(vec![page(
        vec![raw(12, Some("engineering"), None, "invalid")],
        None,
    )]);
    assert!(matches!(
        plan(&out_of_range, &stored, 12).await,
        Err(Error::PageOutOfRange { .. })
    ));

    let duplicate = FakeLog::new(vec![
        page(vec![raw(11, Some("engineering"), None, "first")], Some(11)),
        page(vec![raw(11, Some("engineering"), None, "again")], None),
    ]);
    assert!(matches!(
        plan(&duplicate, &stored, 12).await,
        Err(Error::DuplicateSequence { .. })
    ));

    let empty_cursor = FakeLog::new(vec![page(Vec::new(), Some(11))]);
    assert!(matches!(
        plan(&empty_cursor, &stored, 12).await,
        Err(Error::EmptyPageCursor { .. })
    ));

    let stalled = FakeLog::new(vec![page(
        vec![raw(11, Some("engineering"), None, "row")],
        Some(12),
    )]);
    assert!(matches!(
        plan(&stalled, &stored, 12).await,
        Err(Error::CursorDidNotAdvance { .. })
    ));

    let after_oldest = FakeLog::new(vec![page(
        vec![
            raw(11, Some("engineering"), None, "newer"),
            raw(10, Some("engineering"), None, "older"),
        ],
        Some(11),
    )]);
    assert!(matches!(
        plan(&after_oldest, &stored, 12).await,
        Err(Error::CursorAfterOldest { .. })
    ));

    let too_many = (87..=599)
        .rev()
        .map(|sequence| raw(sequence, Some("engineering"), None, "row"))
        .collect();
    let oversized = FakeLog::new(vec![page(too_many, None)]);
    assert!(matches!(
        plan(&oversized, &stored, 600).await,
        Err(Error::PageTooLarge {
            requested: PAGE_SIZE,
            actual: 513
        })
    ));
}

#[tokio::test]
async fn retry_from_uncommitted_state_repeats_the_same_delta() {
    let pages = vec![page(
        vec![
            raw(11, Some("engineering"), None, "new"),
            raw(10, Some("engineering"), None, "old"),
        ],
        None,
    )];
    let state = state(10);
    let first = plan(&FakeLog::new(pages.clone()), &state, 12)
        .await
        .expect("first");
    let second = plan(&FakeLog::new(pages), &state, 12).await.expect("retry");
    assert_eq!(first, second);
    assert_eq!(state.watermark, Sequence(10));
}

#[tokio::test]
async fn simulated_compare_and_swap_commits_only_the_winning_next_state() {
    let pages = vec![page(
        vec![
            raw(11, Some("engineering"), None, "new"),
            raw(10, Some("engineering"), None, "old"),
        ],
        None,
    )];
    let stored = Arc::new(Mutex::new(state(10)));
    let snapshot = stored.lock().expect("state lock").clone();
    let proposed = delta(
        plan(&FakeLog::new(pages), &snapshot, 12)
            .await
            .expect("plans"),
    )
    .next_state;
    note_present(&mut stored.lock().expect("state lock"), Sequence(20)).expect("concurrent update");
    let mut guard = stored.lock().expect("state lock");
    let won = *guard == snapshot;
    if won {
        *guard = proposed;
    }
    assert!(!won);
    assert_eq!(guard.watermark, Sequence(10));
    assert!(guard.present_above_watermark.contains(&Sequence(20)));
}

// ---------------------------------------------------------------------------
// Private asides
// ---------------------------------------------------------------------------
