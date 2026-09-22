//! Conversations: opened by an ask, run first, concluded to the asker; the silent askee; what is said inside one.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::support::{
    Journal, ask, broadcast, complete, door, hive, policy, post, run, seats, two_seat, wave,
};
use crate::CompletionDriver;
use crate::conduct::{ConductPolicy, Conductor, Event, Refusal, Step};
use crate::driver::{BroadcastRouting, Channel};
use tinyhivemind::Sequence;
use tinyhivemind::speech::{ToolCall, Utterance};

#[test]
fn an_ask_opens_a_conversation_that_runs_first_and_concludes_to_the_asker() {
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

    let asked = wave(
        &mut conductor,
        &journal,
        &[("one", vec![ask("two", "what is the port?")])],
    )
    .expect("wave");
    let root = Sequence(2);
    assert!(matches!(
        asked.events.as_slice(),
        [Event::Asked { seat, askee, root: at }] if seat == "one" && askee == "two" && *at == root
    ));
    assert!(!conductor.finished(), "a conversation is open");

    // The seat asked runs first, in the thread; the asker, held, does not run
    // on the desk. It answers by completing.
    let answered = wave(
        &mut conductor,
        &journal,
        &[("two", vec![complete("port 8080")])],
    )
    .expect("wave");
    assert_eq!(
        seats(&answered.turns)[0],
        ("two", Some(root)),
        "the askee in the thread runs first; the asker, woken by its own ask row, after"
    );
    assert_eq!(answered.turns[0].since, root);
    assert!(matches!(
        answered.turns[0].channel,
        Channel::Thread { root: at, ref other, opened_it: false } if at == root && other == "one"
    ));
    assert!(matches!(
        answered.events.as_slice(),
        [Event::Concluded { root: at, asker, askee, forced: false, .. }]
            if *at == root && asker == "one" && askee == "two"
    ));
    assert_eq!(conductor.conversations(), 1);
    let to_asker = journal.private_to("one");
    assert_eq!(
        to_asker,
        vec!["concluded our conversation (thread 2): port 8080"],
        "the answer reaches the asker as a private row"
    );

    // The asker is released: it runs on the desk, is shown the whole
    // conversation once, and completes.
    let turns = conductor.turns().expect("turns");
    let brief = conductor.open_turn(&turns[0], journal.latest(), Vec::new(), |root| {
        journal.thread(root)
    });
    assert_eq!(brief.conversations.len(), 1);
    assert!(brief.conversations[0].concluded);
    assert!(brief.conversations[0].opened_it);
    assert_eq!(brief.conversations[0].other, "two");
    assert_eq!(
        brief.conversations[0].transcript.len(),
        2,
        "the ask and the answer"
    );
    conductor.record(&turns[0], vec![ToolCall::Speak(complete("shipped"))]);
    while let Some(step) = conductor.step().expect("steps") {
        if let Step::Commit(commit) = step {
            let sequence = journal.append(&commit.author, "row", commit.thread, None);
            run(conductor.committed(sequence)).expect("committed");
        }
    }
    assert!(conductor.finished());
    // Shown once: a second desk turn would show nothing again.
    let brief = conductor.open_turn(&turns[0], journal.latest(), Vec::new(), |_| Vec::new());
    assert!(brief.conversations.is_empty());
}

#[test]
fn a_completion_while_a_conversation_is_open_is_refused_and_explained() {
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
    // The asker asks and completes in the same turn: the ask opens the
    // conversation, and the completion is refused because of it.
    let seen = wave(
        &mut conductor,
        &journal,
        &[("one", vec![ask("two", "?"), complete("too soon")])],
    )
    .expect("wave");
    assert!(seen.events.iter().any(|event| matches!(
        event,
        Event::Refused { seat, thread: None, why: Refusal::AwaitingReply { waiting_on }, .. }
            if seat == "one" && waiting_on == &["two".to_owned()]
    )));
    assert!(
        journal
            .private_to("one")
            .iter()
            .any(|body| body.contains("your completion was refused")),
        "{:?}",
        journal.bodies()
    );
}

#[test]
fn a_silent_askee_is_nudged_once_and_then_walled() {
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
            child_turn_wall: 3,
            turn_wall: 60,
        },
        &journal,
    );
    wave(&mut conductor, &journal, &[("one", vec![ask("two", "?")])]).expect("wave");
    // Turn one in the thread: a post, no answer. Told once, owed a turn.
    let posted = wave(&mut conductor, &journal, &[("two", vec![post("thinking")])]).expect("wave");
    assert!(matches!(
        posted.events.as_slice(),
        [Event::Nudged { seat, thread: Some(_) }] if seat == "two"
    ));
    assert!(
        journal
            .thread(Sequence(2))
            .iter()
            .any(|row| row.contains("is waiting"))
    );
    // Turn two: silence. No second nudge; still owed nothing, so the thread
    // runs it once more because the nudge owed it a turn.
    let silent = wave(&mut conductor, &journal, &[]).expect("wave");
    assert_eq!(seats(&silent.turns)[0], ("two", Some(Sequence(2))));
    assert!(
        !silent.events.iter().any(|event| matches!(
            event,
            Event::Nudged {
                thread: Some(_),
                ..
            }
        )),
        "a second silence stands: {:?}",
        silent.events
    );
    // Turn three reaches the wall: concluded without an answer, forced.
    let walled = wave(
        &mut conductor,
        &journal,
        &[("two", vec![post("still thinking")])],
    )
    .expect("wave");
    assert!(
        walled
            .events
            .iter()
            .any(|event| matches!(event, Event::Concluded { forced: true, .. })),
        "{:?}",
        walled.events
    );
    assert!(
        journal
            .private_to("one")
            .iter()
            .any(|body| body.contains("did not conclude in time"))
    );
}

#[test]
fn a_conversation_at_its_wall_concludes_without_a_nudge() {
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
            child_turn_wall: 1,
            turn_wall: 60,
        },
        &journal,
    );
    wave(&mut conductor, &journal, &[("one", vec![ask("two", "?")])]).expect("wave");
    // The first thread turn is the last: the wall is one. It concludes, and
    // the askee is not told to answer a conversation that is already over.
    let walled = wave(&mut conductor, &journal, &[("two", vec![post("hm")])]).expect("wave");
    assert!(
        walled
            .events
            .iter()
            .any(|event| matches!(event, Event::Concluded { forced: true, .. })),
        "{:?}",
        walled.events
    );
    assert!(
        !walled.events.iter().any(|event| matches!(
            event,
            Event::Nudged {
                thread: Some(_),
                ..
            }
        )),
        "{:?}",
        walled.events
    );
    assert!(
        !journal
            .thread(Sequence(2))
            .iter()
            .any(|row| row.contains("is waiting")),
        "{:?}",
        journal.thread(Sequence(2))
    );
}

#[test]
fn a_refused_reply_in_a_conversation_is_not_its_answer() {
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
    wave(&mut conductor, &journal, &[("one", vec![ask("two", "?")])]).expect("wave");
    // The askee completes in the thread before being shown its assignment:
    // the host opened its turn at a watermark below the ask row, so the fold
    // refuses the row.
    conductor.begin_wave();
    let turns = conductor.turns().expect("turns");
    let thread_turn = turns
        .iter()
        .find(|turn| turn.seat == "two")
        .expect("the askee is due");
    conductor.open_turn(thread_turn, Sequence(1), Vec::new(), |_| Vec::new());
    conductor.record(thread_turn, vec![ToolCall::Speak(complete("too early"))]);
    let mut refused = false;
    while let Some(step) = conductor.step().expect("steps") {
        match step {
            Step::Commit(commit) => {
                let sequence = journal.append(&commit.author, "row", commit.thread, None);
                run(conductor.committed(sequence)).expect("committed");
            }
            Step::Event(Event::Refused {
                why: Refusal::NotYetShown,
                ..
            }) => refused = true,
            Step::Event(_) | Step::Note(_) => {}
        }
    }
    assert!(refused, "the early completion was refused");
    // The conversation goes on to conclude without an answer: the refused
    // message was never the conversation's.
    let mut concluded = false;
    for _ in 0..8 {
        let seen = wave(&mut conductor, &journal, &[]).expect("wave");
        if seen
            .events
            .iter()
            .any(|event| matches!(event, Event::Concluded { .. }))
        {
            concluded = true;
            break;
        }
    }
    assert!(concluded, "the conversation concluded");
    assert!(
        journal
            .private_to("one")
            .iter()
            .all(|body| !body.contains("too early")),
        "{:?}",
        journal.private_to("one")
    );
}

#[test]
fn a_broadcast_or_ask_inside_a_conversation_is_desk_work_and_a_dm_is_dropped() {
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
    wave(&mut conductor, &journal, &[("one", vec![ask("two", "?")])]).expect("wave");
    // Inside the thread, two broadcasts to the desk, dms nobody, and answers.
    let seen = wave(
        &mut conductor,
        &journal,
        &[(
            "two",
            vec![
                broadcast("three should check the logs"),
                Utterance::Dm {
                    to: vec!["one".into()],
                    message: "psst".into(),
                },
                complete("answered"),
            ],
        )],
    )
    .expect("wave");
    assert!(
        seen.events
            .iter()
            .any(|event| matches!(event, Event::Broadcast { seat, to, .. } if seat == "two" && !to.is_empty())),
        "{:?}",
        seen.events
    );
    assert!(
        seen.events
            .iter()
            .any(|event| matches!(event, Event::Concluded { .. }))
    );
    assert!(
        !journal.bodies().iter().any(|body| body == "psst"),
        "a dm in a thread is not served, so it is not a row"
    );
    // The broadcast landed on the desk as desk work for a third seat.
    let turns = conductor.turns().expect("turns");
    assert!(
        seats(&turns)
            .iter()
            .any(|(seat, thread)| *seat == "three" && thread.is_none())
    );
}

#[test]
fn nothing_due_concludes_every_open_conversation_without_an_answer() {
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
    wave(&mut conductor, &journal, &[("one", vec![ask("two", "?")])]).expect("wave");
    // The askee is nudged after its first silent turn, runs once more, and
    // then nothing is due: the conversation is forced closed.
    wave(&mut conductor, &journal, &[]).expect("wave");
    wave(&mut conductor, &journal, &[]).expect("wave");
    let forced = wave(&mut conductor, &journal, &[]).expect("wave");
    assert!(forced.turns.is_empty());
    assert!(
        forced
            .events
            .iter()
            .any(|event| matches!(event, Event::Concluded { forced: true, .. })),
        "{:?}",
        forced.events
    );
    assert_eq!(conductor.conversations(), 1);
}

#[test]
fn a_conclusion_the_fold_refuses_leaves_the_conversation_to_conclude_later() {
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
    let asked = wave(&mut conductor, &journal, &[("one", vec![ask("two", "?")])]).expect("wave");
    let root = asked.commits[0].0;

    // The askee answers, and the host reports the conclusion's row at a
    // sequence the episode already holds: the fold refuses it.
    conductor.begin_wave();
    let turns = conductor.turns().expect("turns");
    for turn in &turns {
        conductor.open_turn(turn, journal.latest(), Vec::new(), |root| {
            journal.thread(root)
        });
        if turn.seat == "two" {
            conductor.record(turn, vec![ToolCall::Speak(complete("port 8080"))]);
        }
    }
    let mut refused = false;
    loop {
        match conductor.step() {
            Ok(None) => break,
            Ok(Some(Step::Commit(commit))) => {
                let sequence = if matches!(commit.utterance, Utterance::Dm { .. }) {
                    root
                } else {
                    journal.append(
                        &commit.author,
                        "row",
                        commit.thread,
                        commit.only_for.clone(),
                    )
                };
                if run(conductor.committed(sequence)).is_err() {
                    refused = true;
                }
            }
            Ok(Some(_)) => {}
            Err(error) => panic!("{error}"),
        }
    }
    assert!(refused, "a reused sequence is refused by the fold");
    assert_eq!(conductor.conversations(), 0, "nothing concluded");
    assert!(!conductor.finished(), "the conversation is still open");

    // The next wave concludes it, at a row the host gives properly.
    let later = wave(&mut conductor, &journal, &[]).expect("wave");
    assert!(
        later
            .events
            .iter()
            .any(|event| matches!(event, Event::Concluded { root: at, .. } if *at == root)),
        "{:?}",
        later.events
    );
    assert_eq!(conductor.conversations(), 1);
}
