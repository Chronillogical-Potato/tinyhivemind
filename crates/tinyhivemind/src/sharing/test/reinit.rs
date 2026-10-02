//! Reinitialization tests: `Conversation::equivalent_to`, and the three
//! conditions under which `prepare_delta` gives up on an incremental delta
//! and asks for a full P4 initialization instead.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, conversation, delta, engineering, page, plan, raw, state};
use crate::Error;
use tinyhivemind_core::aside::Viewer;

#[test]
fn conversation_equivalence_accepts_general_aliases_and_exact_named_identity() {
    assert!(
        conversation("general", "General", None)
            .equivalent_to(&conversation("main", "Anything", None))
    );
    assert!(engineering().equivalent_to(&conversation("engineering", "Renamed", None)));
    assert!(!engineering().equivalent_to(&conversation("engineering-2", "Engineering 2", None)));
    assert!(!engineering().equivalent_to(&conversation("engineering-2", "Engineering", None)));
    assert!(!engineering().equivalent_to(&conversation("engineering", "Engineering", Some(1))));
}

#[tokio::test]
async fn changed_current_or_stored_conversation_reinitializes_without_reading() {
    let log = FakeLog::new(Vec::new());
    let desired = engineering();
    let current = conversation("sales", "Sales", None);
    let state = state(10);
    let result = prepare_delta(
        &log,
        &SharingQuery {
            desired_conversation: &desired,
            current_conversation: &current,
            state: &state,
            before: Sequence(11),
            viewer: &Viewer::Operator,
        },
    )
    .await
    .expect("plans");
    assert_eq!(
        result,
        SharingPlan::Reinitialize {
            reason: ReinitializeReason::ConversationChanged
        }
    );
    assert_eq!(log.calls(), 0);

    let mut mismatched = state;
    mismatched.conversation = current;
    assert_eq!(
        plan(&log, &mismatched, 11).await.expect("plans"),
        SharingPlan::Reinitialize {
            reason: ReinitializeReason::ConversationChanged
        }
    );
}

#[tokio::test]
async fn regressing_bound_is_typed_and_performs_no_read() {
    let log = FakeLog::new(Vec::new());
    assert!(matches!(
        plan(&log, &state(10), 9).await,
        Err(Error::WatermarkRegression {
            before: Sequence(9),
            watermark: Sequence(10)
        })
    ));
    assert_eq!(log.calls(), 0);
}

#[tokio::test]
async fn equal_bound_is_empty_and_keeps_state_without_reading() {
    let log = FakeLog::new(Vec::new());
    let state = state(10);
    let result = delta(plan(&log, &state, 10).await.expect("plans"));
    assert!(result.messages.is_empty(), "{:?}", result.messages);
    assert_eq!(result.next_state, state);
    assert_eq!(log.calls(), 0);
}

#[tokio::test]
async fn exhaustion_above_watermark_requests_reinitialization() {
    let log = FakeLog::new(vec![page(
        vec![raw(11, Some("engineering"), None, "new")],
        None,
    )]);
    assert_eq!(
        plan(&log, &state(10), 12).await.expect("plans"),
        SharingPlan::Reinitialize {
            reason: ReinitializeReason::WatermarkUnavailable
        }
    );
}

#[tokio::test]
async fn scan_cap_above_watermark_requests_gap_reinitialization() {
    let rows: Vec<_> = (1001..=3048)
        .rev()
        .map(|sequence| raw(sequence, Some("sales"), None, "other"))
        .collect();
    let mut pages = Vec::new();
    for chunk in rows.chunks(PAGE_SIZE) {
        pages.push(page(chunk.to_vec(), chunk.last().map(|row| row.sequence.0)));
    }
    let log = FakeLog::new(pages);
    assert_eq!(
        plan(&log, &state(10), 3049).await.expect("plans"),
        SharingPlan::Reinitialize {
            reason: ReinitializeReason::GapTooLarge
        }
    );
}
