//! A broadcast refused for a full queue: the refused seat is not run, and the
//! author keeps the work rather than having it closed.

use crate::driver::engine::CompletionDriver;

use super::ledger::{FirstRouter, apply, broadcast};
use super::{episode, hive};

#[test]
fn a_refused_broadcast_neither_runs_the_seat_nor_closes_the_authors_assignment() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 4)
        .expect("driver")
        .with_queue_depth(1)
        .expect("depth");
    let router = FirstRouter::default();
    let state = driver
        .start(episode(&["one", "two", "three"]))
        .expect("state");
    // `two` hands to the busy `one`, filling its one-deep queue.
    let filled = apply(&driver, &state, "two", 1, broadcast("a"), &router)
        .expect("fills the queue")
        .state;
    assert_eq!(filled.ledger().queue_len("one"), 1);
    // `three` is still working and hands to the same full queue.
    let refused = apply(&driver, &filled, "three", 2, broadcast("b"), &router).expect("folds");
    assert!(
        refused.actions.is_empty(),
        "a refused seat is not run: {:?}",
        refused.actions
    );
    assert_eq!(refused.state.ledger().queue_len("one"), 1, "nothing queued");
    assert!(
        refused
            .state
            .episode()
            .participants
            .iter()
            .any(|participant| participant.agent_id == "three" && participant.is_pending()),
        "the author keeps its open assignment",
    );
    assert_eq!(
        refused.state.seen().ran_for.get("three"),
        None,
        "and is owed another turn"
    );
}
