//! What ties a row to its conversation and an event to its row, as a host
//! reads them back to draw the desk.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::support::{
    ClarifyRouter, Journal, ask, broadcast, complete, door, hive, policy, post, two_seat, wave,
};
use crate::CompletionDriver;
use crate::conduct::{ConductPolicy, Conductor, Event, Refusal};
use crate::driver::BroadcastRouting;
use tinyhivemind::Sequence;
use tinyhivemind::speech::Utterance;

#[test]
fn every_row_of_a_conversation_carries_its_root_wherever_it_lands() {
    let hive = hive(&["one", "two", "three"]);
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
    let mut conductor = Conductor::open(
        &driver,
        routing,
        ConductPolicy::default(),
        door(&["one", "two", "three"], &["one"], &journal),
    )
    .expect("opens");

    let opened = wave(
        &mut conductor,
        &journal,
        &[(
            "one",
            vec![post("looking into it"), ask("two", "which port?")],
        )],
    )
    .expect("wave");
    let (posted_at, posted) = &opened.commits[0];
    assert_eq!(
        posted.conversation, None,
        "a desk row belongs to no conversation"
    );
    assert_eq!(*posted_at, Sequence(2));
    let (root, asking) = &opened.commits[1];
    assert!(matches!(asking.utterance, Utterance::Ask { .. }));
    assert_eq!(
        asking.conversation, None,
        "the ask is a desk row; its own sequence is the conversation"
    );
    assert!(opened.events.iter().any(|event| matches!(
        event,
        Event::Asked { root: at, .. } if at == root
    )));

    let talked = wave(
        &mut conductor,
        &journal,
        &[(
            "two",
            vec![
                post("checking the config"),
                broadcast("three should check the logs"),
                complete("port 8080"),
            ],
        )],
    )
    .expect("wave");
    for (_, commit) in &talked.commits {
        assert_eq!(
            commit.conversation,
            Some(*root),
            "{:?} belongs to the conversation",
            commit.utterance
        );
    }
    let lifted = talked
        .commits
        .iter()
        .find(|(_, commit)| commit.utterance.broadcasting())
        .map(|(_, commit)| commit)
        .expect("the broadcast was committed");
    assert_eq!(lifted.thread, None, "desk work lands on the desk");
    let in_thread: Vec<_> = talked
        .commits
        .iter()
        .filter(|(_, commit)| commit.thread == Some(*root))
        .collect();
    assert_eq!(
        in_thread.len(),
        2,
        "the post and the answer are in the thread"
    );
    let (concluded_at, conclusion) = talked
        .commits
        .iter()
        .find(|(_, commit)| matches!(commit.utterance, Utterance::Dm { .. }))
        .expect("the conclusion reached the asker");
    assert_eq!(conclusion.thread, None);
    assert_eq!(conclusion.only_for.as_deref(), Some("one"));
    assert!(talked.events.iter().any(|event| matches!(
        event,
        Event::Concluded { root: r, at, forced: false, .. } if r == root && at == concluded_at
    )));
}

#[test]
fn a_broadcast_event_names_the_row_it_placed_and_the_handoff_its_origin() {
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
    let mut conductor = Conductor::open(
        &driver,
        routing,
        ConductPolicy::default(),
        door(&["one", "two"], &["one", "two"], &journal),
    )
    .expect("opens");
    let placed = wave(
        &mut conductor,
        &journal,
        &[("one", vec![broadcast("two: also check the cache")])],
    )
    .expect("wave");
    let (broadcast_at, _) = placed.commits[0];
    for event in &placed.events {
        match event {
            Event::Broadcast { at, .. } | Event::CompletedByBroadcast { at, .. } => {
                assert_eq!(*at, broadcast_at);
            }
            _ => {}
        }
    }
    assert!(
        placed
            .events
            .iter()
            .any(|event| matches!(event, Event::CompletedByBroadcast { .. }))
    );
    let handed = wave(&mut conductor, &journal, &[("two", vec![complete("mine")])]).expect("wave");
    assert!(handed.events.iter().any(|event| matches!(
        event,
        Event::Handoff { origin, .. } if *origin == broadcast_at
    )));
}

#[test]
fn a_refused_unplaced_or_discharged_row_is_named_by_its_sequence() {
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
    let seen = wave(
        &mut conductor,
        &journal,
        &[("one", vec![broadcast("first"), broadcast("second")])],
    )
    .expect("wave");
    let first = seen.commits[0].0;
    let second = seen.commits[1].0;
    assert!(seen.events.iter().any(|event| matches!(
        event,
        Event::Unplaced { at, .. } if *at == first
    )));
    assert!(seen.events.iter().any(|event| matches!(
        event,
        Event::Discharged { at, .. } if *at == second
    )));

    // A completion refused while a conversation is open names its own row.
    let hive = super::support::hive(&["one", "two"]);
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
    let seen = wave(
        &mut conductor,
        &journal,
        &[("one", vec![ask("two", "?"), complete("too soon")])],
    )
    .expect("wave");
    let completion_at = seen
        .commits
        .iter()
        .find(|(_, commit)| matches!(commit.utterance, Utterance::CompleteEpisode { .. }))
        .map(|(at, _)| *at)
        .expect("the completion was committed");
    assert!(seen.events.iter().any(|event| matches!(
        event,
        Event::Refused { why: Refusal::AwaitingReply { .. }, at, .. } if *at == completion_at
    )));
}
