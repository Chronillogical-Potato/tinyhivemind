//! A parked seat: held on the host, not nudged, not stalled, not proposed,
//! and back where it stopped once released.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::support::{
    Journal, ask, broadcast, complete, hive, policy, run, seats, two_seat, wave, wave_parking,
};
use crate::conduct::{ConductPolicy, Conductor, Event};
use crate::driver::BroadcastRouting;
use crate::test_support::Seat;
use crate::{CompletionDriver, Error};
use serde_json::{Value, json};
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

#[test]
fn a_snapshot_is_taken_between_waves_and_carries_the_episode_on() {
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
    // One asks two, and two parks: a conversation open and a seat held is
    // the most state a snapshot has to carry.
    wave(
        &mut conductor,
        &journal,
        &[("one", vec![ask("two", "may I ship?")])],
    )
    .expect("wave");
    wave_parking(&mut conductor, &journal, &[], &["two"]).expect("wave");
    let root = Sequence(2);
    let snapshot = conductor.snapshot().expect("the wave settled");
    assert_eq!(snapshot.chat, "engineering");
    assert_eq!(conductor.parked(), vec!["two".to_owned()]);
    let turns_before = conductor.turns_run();

    // Through the wire and back, onto a driver the host supplies again.
    let wire = serde_json::to_string(&snapshot).expect("serializes");
    let restored: crate::ConductorState = serde_json::from_str(&wire).expect("deserializes");
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let mut resumed =
        Conductor::resume(&driver, routing, ConductPolicy::default(), restored).expect("resumes");
    assert_eq!(resumed.turns_run(), turns_before, "the count carries");
    assert_eq!(
        resumed.parked(),
        vec!["two".to_owned()],
        "the held seat is still held"
    );
    // Released, it answers in the conversation that was open before the
    // restart, and the episode concludes it.
    resumed.resume_seat("two");
    let answered = wave(
        &mut resumed,
        &journal,
        &[("two", vec![complete("ship it")])],
    )
    .expect("wave");
    assert_eq!(seats(&answered.turns)[0], ("two", Some(root)));
    assert_eq!(
        resumed.conversations(),
        1,
        "the conversation survived the restart and concluded"
    );
}

#[test]
fn a_snapshot_is_refused_only_while_the_host_holds_an_unreported_commit() {
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
    assert!(
        conductor.snapshot().is_some(),
        "a fresh conductor is between waves"
    );
    for step in conductor.begin_wave() {
        let _ = step;
    }
    let turns = conductor.turns().expect("turns");
    conductor.record(
        &turns[0],
        [tinyhivemind::speech::ToolCall::Speak(complete("done"))],
    );
    // Mid-wave, before any commit leaves: the wave travels with the
    // snapshot, so this is recordable.
    let held = conductor
        .snapshot()
        .expect("a wave in progress is still recordable");
    assert!(
        !held.mid_wave_is_empty(),
        "the snapshot carries what the wave has not committed yet"
    );
    // The host now holds a commit whose sequence it has not reported. The
    // conductor cannot say whether that row landed, so it will not write a
    // claim either way.
    let step = conductor.step().expect("a step");
    assert!(matches!(step, Some(crate::Step::Commit(_))));
    assert!(
        conductor.snapshot().is_none(),
        "a commit is out there and unreported"
    );
    // Reported, and it is recordable again.
    let sequence = journal.append("one", "COMPLETE: done", None, Vec::new());
    run(conductor.committed(sequence)).expect("committed");
    assert!(conductor.snapshot().is_some());
}

#[test]
fn a_held_seat_keeps_the_episode_open_even_where_its_work_closed() {
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
    // One completes and parks in the same turn: its work is closed, so the
    // desk is quiescent, but the host is still holding it.
    wave_parking(
        &mut conductor,
        &journal,
        &[("one", vec![complete("done")])],
        &["one"],
    )
    .expect("wave");
    assert_eq!(conductor.parked(), vec!["one".to_owned()]);
    assert!(
        !conductor.finished(),
        "a held seat is not a finished one: the operator's answer needs a \
         loop to come back to"
    );
    // Released, there is nothing left to hold and the episode is over.
    conductor.resume_seat("one");
    assert!(conductor.parked().is_empty());
    assert!(conductor.finished());
}

/// A snapshot with one conversation open and one seat held, as a value a
/// test can bend before handing it back.
fn snapshot_with_a_conversation(journal: &Journal, conductor: &mut Conductor<'_, Seat>) -> Value {
    wave(
        conductor,
        journal,
        &[("one", vec![ask("two", "which port?")])],
    )
    .expect("wave");
    serde_json::to_value(conductor.snapshot().expect("recordable")).expect("serializes")
}

#[test]
fn a_snapshot_that_disagrees_with_itself_is_refused_rather_than_resumed() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = || BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = two_seat(&driver, routing(), ConductPolicy::default(), &journal);
    let good = snapshot_with_a_conversation(&journal, &mut conductor);
    // The unbent snapshot resumes, so each refusal below is about the bend.
    let restored: crate::ConductorState =
        serde_json::from_value(good.clone()).expect("deserializes");
    assert!(Conductor::resume(&driver, routing(), ConductPolicy::default(), restored).is_ok());

    // A conversation filed under a root it does not call its own.
    let mut bent = good.clone();
    bent["children"][0][0] = json!(99);
    let restored: crate::ConductorState = serde_json::from_value(bent).expect("deserializes");
    let refused = Conductor::resume(&driver, routing(), ConductPolicy::default(), restored);
    assert!(
        matches!(&refused, Err(Error::InconsistentSnapshot { reason }) if reason.contains("99")),
        "{refused:?}"
    );

    // A cursor past the conversations it points into: shown_conversations
    // would index off the end of the list the first time that seat spoke.
    let mut bent = good.clone();
    bent["shown"] = json!({ "one": 7 });
    let restored: crate::ConductorState = serde_json::from_value(bent).expect("deserializes");
    let refused = Conductor::resume(&driver, routing(), ConductPolicy::default(), restored);
    assert!(
        matches!(&refused, Err(Error::InconsistentSnapshot { reason }) if reason.contains("@one")),
        "{refused:?}"
    );

    // A seat held that this desk does not seat: nothing could ever release
    // it into a wave.
    let mut bent = good.clone();
    bent["parked"] = json!({ "nobody": null });
    let restored: crate::ConductorState = serde_json::from_value(bent).expect("deserializes");
    let refused = Conductor::resume(&driver, routing(), ConductPolicy::default(), restored);
    assert!(
        matches!(&refused, Err(Error::InconsistentSnapshot { reason }) if reason.contains("nobody")),
        "{refused:?}"
    );

    // A conversation whose own state names another desk is refused by the
    // driver, exactly as the desk episode's state would be.
    let mut bent = good;
    bent["children"][0][1]["state"]["episode"]["conversation"]["desk_id"] = json!("marketing");
    let restored: crate::ConductorState = serde_json::from_value(bent).expect("deserializes");
    let refused = Conductor::resume(&driver, routing(), ConductPolicy::default(), restored);
    assert!(
        matches!(&refused, Err(Error::OutOfHiveEpisode { .. })),
        "{refused:?}"
    );
}
