//! `initialized_state` and `note_present` tests: watermark seeding, and the
//! present-above-watermark bookkeeping used to record a row already
//! accepted through a concurrent path.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{FakeLog, engineering, plan, state};
use crate::runtime::Error;
use std::collections::BTreeSet;

#[test]
fn initialized_state_starts_empty_at_the_accepted_trigger() {
    let state = initialized_state(engineering(), Sequence(10));
    assert_eq!(state.watermark, Sequence(10));
    assert!(
        state.present_above_watermark.is_empty(),
        "{:?}",
        state.present_above_watermark
    );
}

#[test]
fn note_present_ignores_old_rows_and_is_idempotent_for_future_rows() {
    let mut state = state(10);
    note_present(&mut state, Sequence(9)).expect("old no-op");
    note_present(&mut state, Sequence(10)).expect("watermark no-op");
    note_present(&mut state, Sequence(11)).expect("future insert");
    note_present(&mut state, Sequence(11)).expect("future idempotent");
    assert_eq!(
        state.present_above_watermark,
        BTreeSet::from([Sequence(11)])
    );
    assert_eq!(state.watermark, Sequence(10));
}

#[test]
fn note_present_overflow_is_typed_and_atomic() {
    let mut state = state(10);
    for sequence in 11..=10 + PRESENT_SET_LIMIT as u64 {
        note_present(&mut state, Sequence(sequence)).expect("within bound");
    }
    let before = state.clone();
    assert!(matches!(
        note_present(&mut state, Sequence(1000)),
        Err(Error::PresentSetOverflow {
            limit: 64,
            sequence: Sequence(1000)
        })
    ));
    assert_eq!(state, before);
}

#[test]
fn note_present_accepts_a_duplicate_when_the_present_set_is_full() {
    let mut state = state(10);
    for sequence in 11..=10 + PRESENT_SET_LIMIT as u64 {
        note_present(&mut state, Sequence(sequence)).expect("within bound");
    }
    let before = state.clone();
    note_present(&mut state, Sequence(11)).expect("duplicate remains idempotent");
    assert_eq!(state, before);
}

#[test]
fn note_present_rejects_an_already_oversized_manual_state_atomically() {
    let mut state = state(10);
    state.present_above_watermark = (11..=11 + PRESENT_SET_LIMIT as u64).map(Sequence).collect();
    let before = state.clone();
    assert!(matches!(
        note_present(&mut state, Sequence(11)),
        Err(Error::PresentSetTooLarge {
            limit: 64,
            actual: 65
        })
    ));
    assert_eq!(state, before);
}

#[tokio::test]
async fn prepare_delta_rejects_an_oversized_manual_state_without_reading() {
    let log = FakeLog::new(Vec::new());
    let mut oversized = state(10);
    oversized.present_above_watermark =
        (11..=11 + PRESENT_SET_LIMIT as u64).map(Sequence).collect();
    assert!(matches!(
        plan(&log, &oversized, 11).await,
        Err(Error::PresentSetTooLarge {
            limit: 64,
            actual: 65
        })
    ));
    assert_eq!(log.calls(), 0);
}
