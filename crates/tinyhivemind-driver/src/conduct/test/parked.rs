//! A parked seat: held on the host, not nudged, not stalled, not proposed,
//! and back where it stopped once released.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::support::{
    Journal, ask, broadcast, complete, hive, policy, seats, two_seat, wave, wave_parking,
};
use crate::conduct::{ConductPolicy, Conductor, Event};
use crate::driver::BroadcastRouting;
use crate::{CompletionDriver, Error};
use tinyhivemind::Sequence;

#[test]
fn a_parked_desk_seat_is_held_until_the_host_releases_it() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = two_seat(&driver, routing, ConductPolicy::default(), &journal);
    // One's turn stops on the host: an approval, say.
    let parked = wave_parking(&mut conductor, &journal, &[], &["one"]).expect("wave");
    assert_eq!(seats(&parked.turns), vec![("one", None)]);
    assert!(matches!(
        parked.events.as_slice(),
        [Event::Parked { seat, thread: None }] if seat == "one"
    ));
    assert_eq!(conductor.parked(), vec!["one".to_owned()]);
    assert_eq!(conductor.turns_run(), 1, "a parked turn ran");
    assert!(!conductor.finished());
    // The next wave nudges nobody, proposes nothing, and is not a stall:
    // the seat is waiting on the host, and the host is asked.
    let waiting = wave(&mut conductor, &journal, &[]).expect("not a stall");
    assert!(waiting.turns.is_empty());
    assert!(waiting.events.is_empty(), "{:?}", waiting.events);
    assert!(
        !journal
            .bodies()
            .iter()
            .any(|body| body.contains("open work")),
        "a parked seat is not told it is silent"
    );
    // Released, it is owed its turn where it parked, and completes.
    conductor.resume_seat("one");
    let resumed = wave(
        &mut conductor,
        &journal,
        &[("one", vec![complete("approved and done")])],
    )
    .expect("wave");
    assert_eq!(seats(&resumed.turns), vec![("one", None)]);
    assert!(matches!(
        resumed.events.as_slice(),
        [Event::Resumed { seat, thread: None }] if seat == "one"
    ));
    assert!(conductor.parked().is_empty());
    assert!(conductor.finished());
}

#[test]
fn a_parked_askee_holds_its_conversation_open_and_answers_once_released() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = two_seat(&driver, routing, ConductPolicy::default(), &journal);
    wave(
        &mut conductor,
        &journal,
        &[("one", vec![ask("two", "may I ship?")])],
    )
    .expect("wave");
    let root = Sequence(2);
    // Two parks in the thread; one, woken by its own ask, says nothing.
    let parked = wave_parking(&mut conductor, &journal, &[], &["two"]).expect("wave");
    assert_eq!(
        seats(&parked.turns),
        vec![("two", Some(root)), ("one", None)]
    );
    assert!(
        parked
            .events
            .iter()
            .any(|event| matches!(event, Event::Parked { seat, thread: Some(at) } if seat == "two" && *at == root))
    );
    assert!(
        !parked
            .events
            .iter()
            .any(|event| matches!(event, Event::Nudged { seat, .. } if seat == "two")),
        "a parked askee is not a silent one: {:?}",
        parked.events
    );
    assert_eq!(
        conductor.conversations(),
        0,
        "the conversation waits with it"
    );
    // The asker, silent on the desk, is nudged as ever; the parked askee is
    // not proposed, and the conversation is not concluded for want of it.
    let waiting = wave(&mut conductor, &journal, &[]).expect("not a stall");
    assert_eq!(seats(&waiting.turns), vec![("one", None)]);
    assert!(
        waiting
            .events
            .iter()
            .all(|event| matches!(event, Event::Nudged { seat, thread: None } if seat == "one")),
        "{:?}",
        waiting.events
    );
    assert_eq!(conductor.conversations(), 0);
    conductor.resume_seat("two");
    let answered = wave(
        &mut conductor,
        &journal,
        &[("two", vec![complete("ship it")])],
    )
    .expect("wave");
    assert_eq!(seats(&answered.turns)[0], ("two", Some(root)));
    assert_eq!(
        conductor.conversations(),
        1,
        "answered, the conversation concluded"
    );
}

#[test]
fn releasing_a_seat_that_is_not_parked_changes_nothing_and_a_stall_is_still_a_stall() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor: Conductor<'_, _> =
        two_seat(&driver, routing, ConductPolicy::default(), &journal);
    conductor.resume_seat("one");
    let first = wave(&mut conductor, &journal, &[]).expect("wave");
    assert!(first.events.is_empty(), "no release of a seat never parked");
    // Silent twice with nobody parked: the stall stands.
    wave(&mut conductor, &journal, &[]).expect("nudged");
    let stalled = wave(&mut conductor, &journal, &[]);
    assert!(matches!(stalled, Err(Error::Stalled { seats }) if seats == ["one"]));
}

#[test]
fn what_a_seat_said_before_it_parked_is_recorded() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = two_seat(&driver, routing, ConductPolicy::default(), &journal);
    // One hands work off and then stops on the host: the broadcast lands.
    let parked = wave_parking(
        &mut conductor,
        &journal,
        &[("one", vec![broadcast("someone take the migration")])],
        &["one"],
    )
    .expect("wave");
    assert!(
        journal
            .bodies()
            .iter()
            .any(|body| body.contains("take the migration")),
        "{:?}",
        journal.bodies()
    );
    assert!(
        parked
            .events
            .iter()
            .any(|event| matches!(event, Event::Parked { seat, .. } if seat == "one"))
    );
    assert_eq!(conductor.parked(), vec!["one".to_owned()]);
}
