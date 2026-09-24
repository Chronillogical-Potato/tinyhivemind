//! An episode from its door to quiescence, and one that stalls: what the
//! journal and the runner saw of each.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::json;
use tinyhivemind::Sequence;
use tinyhivemind_driver::{BroadcastRouting, CompletionDriver, ConductPolicy, Event};

use super::super::{Report, run_episode};
use super::support::{ScriptRunner, TestJournal, ask, complete, door, hive, policy, post, run};
use crate::Error;
use crate::runner::{Lane, TurnResult};

#[test]
fn an_episode_runs_from_its_door_to_quiescence_over_the_journal() {
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
    // One asks two (row 2 roots the conversation) and, woken by its own ask
    // row, says nothing; two tries a post, which is not served, and answers
    // in the thread with a completion; one, released by the conclusion,
    // completes on the desk.
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[
            (
                "one",
                vec![
                    vec![ask("two", "which port?", None)],
                    vec![],
                    vec![complete("fixed", None)],
                ],
            ),
            (
                "two",
                vec![vec![post("checking", 2), complete("port 8080", Some(2))]],
            ),
        ],
    );
    let report = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ))
    .expect("the episode settles");
    assert_eq!(
        report,
        Report {
            turns: 4,
            waves: 3,
            discharged: 0,
            conversations: 1,
            settled: 2,
        },
        "{:?}",
        journal.log.all()
    );
    let bodies: Vec<String> = journal
        .log
        .all()
        .iter()
        .map(|row| row.body.clone())
        .collect();
    assert_eq!(bodies[0], "state the root cause");
    assert!(bodies.contains(&"which port?".to_owned()));
    assert!(bodies.contains(&"port 8080".to_owned()));
    assert!(
        bodies
            .iter()
            .any(|body| body.contains("concluded our conversation"))
    );
    assert!(bodies.contains(&"fixed".to_owned()));
    let events = journal.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Asked { root, .. } if *root == Sequence(2)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Concluded { forced: false, .. }))
    );

    the_journal_saw_each_turn(&journal, &runner);
}

/// The thread turn was briefed with the thread, the asker's waking desk
/// turn was shown the concluded conversation, and every turn came back to
/// the journal with what it recorded.
fn the_journal_saw_each_turn(journal: &TestJournal, runner: &ScriptRunner) {
    let prompts = runner.prompts();
    let (_, lane, thread_prompt) = prompts
        .iter()
        .find(|(seat, _, _)| seat == "two")
        .expect("two ran");
    assert_eq!(*lane, Lane::Thread(Sequence(2)));
    assert!(thread_prompt.contains("which port?"), "{thread_prompt}");
    // **The asker reads the answer, and reads it once.**
    //
    // It arrives in the desk read -- the reply under the ask, promoted to
    // channel level for the seat the thread was confided to, and marked
    // there as private so the asker cannot mistake it for something said in
    // the open. Because the desk read already carries it, no
    // `ConversationView` is built for that conversation: the same lines
    // under a heading of their own would be the second copy.
    let desk_prompts: Vec<&String> = prompts
        .iter()
        .filter(|(seat, lane, _)| seat == "one" && *lane == Lane::Desk)
        .map(|(_, _, prompt)| prompt)
        .collect();
    let carried: Vec<&&String> = desk_prompts
        .iter()
        .filter(|prompt| prompt.contains("port 8080"))
        .collect();
    assert_eq!(
        carried.len(),
        1,
        "the answer reaches the asker's desk turn exactly once: {desk_prompts:?}"
    );
    assert!(
        carried[0].contains("@two (privately): port 8080"),
        "and says it was confided, not said on the desk: {}",
        carried[0]
    );
    assert!(
        !carried[0].contains("## Conversations you had since you last spoke"),
        "no second copy under a heading of its own: {}",
        carried[0]
    );
    // Every turn came back to the journal with what it recorded.
    let turns = journal.turns.lock().unwrap();
    assert_eq!(turns.len(), 4, "three that called, and one's silent turn");
    assert!(
        turns
            .iter()
            .all(|(_, _, outcome, _, _)| matches!(outcome, TurnResult::Replied(_)))
    );
    // `post` is in the vocabulary and not served: two's post in the thread
    // was refused inside its turn, and the journal was told so.
    let refusals: Vec<(&str, usize)> = turns
        .iter()
        .map(|(seat, _, _, refused, _)| (seat.as_str(), *refused))
        .filter(|(_, refused)| *refused > 0)
        .collect();
    assert_eq!(refusals, vec![("two", 1)]);
    assert_eq!(
        turns
            .iter()
            .filter(|(_, _, _, _, recorded)| *recorded >= 1)
            .count(),
        3
    );
}

#[test]
fn a_seat_that_says_nothing_is_nudged_and_then_the_episode_stalls() {
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
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![], vec![("fail", json!({}))]])],
    );
    let stalled = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        door(&journal, &["one", "two"], &["one"]),
    ));
    assert!(
        matches!(&stalled, Err(Error::Conduct(tinyhivemind_driver::Error::Stalled { seats })) if seats == &["one".to_owned()]),
        "{stalled:?}"
    );
    // The nudge reached the journal as a row to one alone, and the failed
    // turn reached it as a turn that failed.
    let rows = journal.log.all();
    assert!(
        rows.iter()
            .any(|row| row.author == "desk" && row.only_for.as_deref() == Some("one"))
    );
    let turns = journal.turns.lock().unwrap();
    assert!(turns.iter().any(|(_, _, outcome, _, recorded)| matches!(
        outcome,
        TurnResult::Failed(_)
    ) && *recorded == 0));
    assert!(
        journal
            .events()
            .iter()
            .any(|event| matches!(event, Event::Nudged { thread: None, .. }))
    );
    // The nudge is one's alone: it woke nobody else, and what two would be
    // shown of the desk does not hold it.
    let prompts = runner.prompts();
    assert!(
        prompts.iter().all(|(seat, _, _)| seat == "one"),
        "{prompts:?}"
    );
    let shown_two = journal.log.desk_since("two", None);
    assert!(!shown_two.is_empty());
    assert!(
        shown_two.iter().all(|row| !row.contains("open work")),
        "{shown_two:?}"
    );
}
