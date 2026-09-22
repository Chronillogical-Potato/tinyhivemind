//! The desk: stalled seats, broadcasts placed and unplaced, handoffs, the budget, the walls, and the commit protocol.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::support::{
    ClarifyRouter, Journal, broadcast, complete, door, hive, policy, post, run, seats, two_seat,
    wave,
};
use crate::conduct::{ConductPolicy, Conductor, Event, Refusal, Step};
use crate::driver::BroadcastRouting;
use crate::{CompletionDriver, Error};
use tinyhivemind::Sequence;
use tinyhivemind::speech::ToolCall;

#[test]
fn a_stalled_desk_seat_is_nudged_once_per_assignment_and_then_the_episode_stalls() {
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
    // One runs and calls nothing. Next wave it is stalled: nudged, owed a turn.
    wave(&mut conductor, &journal, &[]).expect("wave");
    let nudged = wave(&mut conductor, &journal, &[]).expect("wave");
    assert!(matches!(
        nudged.events.as_slice(),
        [Event::Nudged { seat, thread: None }] if seat == "one"
    ));
    assert_eq!(seats(&nudged.turns), vec![("one", None)]);
    assert!(journal.private_to("one")[0].contains("you hold open work"));
    // Silent again, for the same assignment: no second nudge, nothing due.
    let stalled = wave(&mut conductor, &journal, &[]);
    assert!(
        matches!(&stalled, Err(Error::Stalled { seats }) if seats == &["one".to_owned()]),
        "{stalled:?}"
    );
}

#[test]
fn an_unplaced_broadcast_leaves_the_work_with_the_author_and_says_so() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: Some(&ClarifyRouter),
        reasoning: Some(&ClarifyRouter),
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = two_seat(&driver, routing, ConductPolicy::default(), &journal);
    let seen = wave(
        &mut conductor,
        &journal,
        &[("one", vec![broadcast("someone take this")])],
    )
    .expect("wave");
    assert!(matches!(
        seen.events.as_slice(),
        [Event::Unplaced { seat, .. }] if seat == "one"
    ));
    assert!(journal.private_to("one")[0].contains("nobody on this desk can take that"));
    assert!(!conductor.finished(), "the author keeps the work");
}

#[test]
fn a_placed_broadcast_completes_its_author_and_a_busy_recipient_gets_it_as_a_handoff() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4)
        .expect("driver")
        .with_queue_depth(2)
        .expect("depth");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    // Both start: two holds work, so one's broadcast to it is queued.
    let mut conductor = Conductor::open(
        &driver,
        routing,
        ConductPolicy::default(),
        door(&["one", "two"], &["one", "two"], &journal),
    )
    .expect("opens");
    let seen = wave(
        &mut conductor,
        &journal,
        &[("one", vec![broadcast("two: also check the cache")])],
    )
    .expect("wave");
    assert!(
        seen.events.iter().any(|event| matches!(
            event,
            Event::CompletedByBroadcast { seat, .. } if seat == "one"
        )),
        "{:?}",
        seen.events
    );
    // Two completes its own work and is handed the queued broadcast.
    let handed = wave(
        &mut conductor,
        &journal,
        &[("two", vec![complete("mine is done")])],
    )
    .expect("wave");
    assert!(
        handed.events.iter().any(|event| matches!(
            event,
            Event::Handoff { to, from, .. } if to == "two" && from == "one"
        )),
        "{:?}",
        handed.events
    );
    assert!(journal.private_to("two")[0].contains("handoff from @one"));
    assert!(!conductor.finished());
    // A completion before it is shown the handoff is refused and explained;
    // the host shows nothing new by opening the turn at a stale watermark.
    let turns = conductor.turns().expect("turns");
    assert_eq!(seats(&turns), vec![("two", None)]);
    conductor.record(&turns[0], vec![ToolCall::Speak(complete("that too"))]);
    let mut refused = None;
    while let Some(step) = conductor.step().expect("steps") {
        match step {
            Step::Commit(commit) => {
                let sequence = journal.append(&commit.author, "row", None, None);
                run(conductor.committed(sequence)).expect("committed");
            }
            Step::Event(Event::Refused { why, .. }) => refused = Some(why),
            Step::Event(_) | Step::Note(_) => {}
        }
    }
    assert!(
        matches!(refused, Some(Refusal::Undelivered { .. })),
        "{refused:?}"
    );
}

#[test]
fn a_spent_broadcast_budget_completes_the_seat_with_the_work() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4)
        .expect("driver")
        .with_broadcast_budget(Some(1));
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: Some(&ClarifyRouter),
        reasoning: Some(&ClarifyRouter),
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = two_seat(&driver, routing, ConductPolicy::default(), &journal);
    // Two unplaced broadcasts from one assignment: the second is over budget.
    let seen = wave(
        &mut conductor,
        &journal,
        &[("one", vec![broadcast("first"), broadcast("second")])],
    )
    .expect("wave");
    assert!(
        seen.events
            .iter()
            .any(|event| matches!(event, Event::Discharged { seat, .. } if seat == "one")),
        "{:?}",
        seen.events
    );
    assert_eq!(conductor.discharged(), 1);
    assert!(
        journal
            .bodies()
            .iter()
            .any(|body| body == "COMPLETE: budget spent; keeping the work")
    );
    assert!(conductor.finished());
}

#[test]
fn the_turn_wall_ends_the_episode() {
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
    let mut conductor = two_seat(
        &driver,
        routing,
        ConductPolicy {
            child_turn_wall: 6,
            turn_wall: 1,
        },
        &journal,
    );
    let walled = wave(&mut conductor, &journal, &[("one", vec![post("hm")])]);
    assert!(
        matches!(walled, Err(Error::TurnWall { wall: 1 })),
        "{walled:?}"
    );
}

#[test]
fn a_commit_must_be_reported_before_the_next_step_and_only_once() {
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
    assert!(matches!(
        run(conductor.committed(Sequence(9))),
        Err(Error::NoCommitOutstanding)
    ));
    conductor.begin_wave();
    let turns = conductor.turns().expect("turns");
    conductor.record(&turns[0], vec![ToolCall::Speak(post("a row"))]);
    let step = conductor.step().expect("step").expect("a commit");
    assert!(
        matches!(&step, Step::Commit(commit) if commit.author == "one" && commit.thread.is_none())
    );
    assert!(matches!(conductor.step(), Err(Error::CommitOutstanding)));
    run(conductor.committed(Sequence(2))).expect("committed");
    assert!(conductor.step().expect("settles").is_none());
    assert_eq!(ConductPolicy::default().child_turn_wall, 6);
}
