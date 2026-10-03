//! Hosted runner defaults, including thread-scoped refusals.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;

use openhuman_embed::Runtime;
use tinyhivemind_core::runtime::{SESSION_WINDOW, Sequence};
use tinyhivemind_tools::{Dispatch, EpisodeTools};

use super::{PlainHost, one_turn};
use crate::{HostedRunner, Lane, MemoryLog, SeatRunner, TurnResult};

/// A hosted seat on a host that keeps every default, run once on the desk
/// and once in a thread it is not in: the defaults hold, the thread turn is
/// seeded from the thread, and a call outside its thread is refused.
pub(super) async fn run(runtime: &Arc<Runtime>) {
    let log = MemoryLog::new("engineering");
    log.append("operator", "state the root cause", None, &[]);
    let host = Arc::new(PlainHost {
        log,
        runtime: Arc::clone(runtime),
    });
    let runner = HostedRunner::seat(
        Arc::clone(&host),
        Arc::new(EpisodeTools::new(["lead"])),
        &["lead".to_owned()],
        "engineering",
        "Engineering",
        SESSION_WINDOW,
    )
    .expect("hosted seats");
    assert_eq!(
        runner.tools().display_name("lead"),
        "lead",
        "a host that names nobody leaves the record's seats by id"
    );
    let (reply, events) = one_turn(&runner, host.log.latest()).await;
    assert!(!reply.is_empty(), "{reply:?}");
    assert_eq!(
        events.len(),
        1,
        "the bare-named belt is admitted by default"
    );
    runner.open(
        "lead",
        Vec::new(),
        Dispatch {
            chat: "engineering".into(),
            parent: Some("1".into()),
        },
    );
    let (_, lane, outcome) = runner
        .turn(
            "lead".into(),
            Lane::Thread(Sequence(1)),
            Some(Sequence(1)),
            "In the thread.".into(),
        )
        .await;
    assert_eq!(lane, Lane::Thread(Sequence(1)));
    assert!(matches!(outcome, TurnResult::Replied(_)), "{outcome:?}");
    assert!(
        runner.close("lead").is_empty(),
        "the scripted call names no thread, so the record refused it"
    );
    assert_eq!(runner.tools().drain_refusals("lead").len(), 1);
}
