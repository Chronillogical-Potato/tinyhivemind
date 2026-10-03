//! A seat parked on the host: held, waited for, released; and what its
//! other conversations put in its brief.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::VecDeque;

use tinyhivemind::{Conversation, Sequence};
use tinyhivemind_driver::{BroadcastRouting, CompletionDriver, ConductPolicy, Event};

use super::super::run_episode;
use super::support::{ScriptRunner, TestJournal, complete, door, hive, policy, run};
use crate::Error;

/// A call that makes the scripted turn park rather than reply.
fn park() -> (&'static str, serde_json::Value) {
    ("park", serde_json::Value::Null)
}

#[test]
fn a_parked_seat_waits_for_the_host_and_runs_again_once_released() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = TestJournal::new();
    *journal.release.lock().unwrap() = VecDeque::from([vec!["one".to_owned()]]);
    // One parks on its first turn, then completes on the turn it is given
    // after the host releases it.
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![park()], vec![complete("approved", None)]])],
    );
    let report = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ))
    .expect("the episode runs");
    assert!(report.settled >= 1);
    assert_eq!(report.turns, 2, "the parked turn and the one after it");
    let events = journal.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Parked { seat, thread: None } if seat == "one")),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Resumed { seat, .. } if seat == "one")),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::Nudged { .. })),
        "a parked seat is not nudged: {events:?}"
    );
    // The host was asked about exactly the seats that were parked.
    assert_eq!(*journal.asked.lock().unwrap(), vec![vec!["one".to_owned()]]);
}

#[test]
fn a_host_that_releases_nobody_ends_the_episode_parked() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = TestJournal::new();
    let runner = ScriptRunner::new(&["one", "two"], &[("one", vec![vec![park()]])]);
    let ended = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ));
    assert!(
        matches!(&ended, Err(Error::Conduct(tinyhivemind_driver::Error::Parked { seats })) if seats == &["one".to_owned()]),
        "{ended:?}"
    );
}

#[test]
fn the_seats_other_conversations_reach_its_brief_as_context() {
    let hive = hive(&["one", "two"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy();
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = TestJournal::new();
    // The host keeps one log for both desks, and names them both: the
    // episode's own is skipped, marketing is not.
    journal
        .log
        .append_to("marketing", "three", "launch is friday", None, &[]);
    *journal.channels.lock().unwrap() = vec![
        Conversation {
            desk_id: "engineering".into(),
            desk_name: "Engineering".into(),
            thread_root: None,
        },
        Conversation {
            desk_id: "marketing".into(),
            desk_name: "Marketing".into(),
            thread_root: None,
        },
    ];
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![complete("done", None)]])],
    );
    run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ))
    .expect("the episode runs");
    let prompts = runner.prompts();
    assert_eq!(prompts.len(), 1);
    let prompt = &prompts[0].2;
    assert!(prompt.contains("## Elsewhere, for context"), "{prompt}");
    assert!(
        prompt.contains("### Marketing (marketing)\n@three: launch is friday"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("### Engineering"),
        "the turn's own desk is not elsewhere: {prompt}"
    );
    assert_ne!(journal.log.latest(), Some(Sequence(0)));
}
