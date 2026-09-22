//! The wave watermark: a log numbered from zero, and a log that grows under
//! the loop.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::atomic::AtomicBool;

use tinyhivemind::Sequence;
use tinyhivemind_driver::{BroadcastRouting, CompletionDriver, ConductPolicy, Door};

use super::super::run_episode;
use super::support::{GrowingLog, ScriptRunner, TestJournal, complete, door, hive, policy, run};
use crate::journal::MemoryLog;

#[test]
fn a_task_on_a_log_numbered_from_zero_reaches_the_first_turn() {
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
    // A host that numbers its first row zero, as some do: the task is row
    // zero, and nothing sits below it.
    let journal = TestJournal::over(MemoryLog::numbered_from("engineering", Sequence(0)));
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![complete("done", None)]])],
    );
    let entrance = door(&journal, &["one", "two"], &["one"]);
    assert_eq!(entrance.opened_at, Sequence(0));
    let report = run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        entrance,
    ))
    .expect("the episode runs");
    assert_eq!(report.settled, 2);
    assert_eq!(report.turns, 1);
    // The seat's one turn was shown the task, and was shown nothing before.
    let prompts = runner.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].2.contains("state the root cause"),
        "{}",
        prompts[0].2
    );
    assert_eq!(runner.since(), vec![None]);
    // The completion landed above the task, at row one.
    let rows = journal.log.all();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].sequence, Sequence(1));
    assert!(rows[1].body.contains("done"));
}

#[test]
fn a_row_the_host_appends_above_the_wave_watermark_is_shown_once_and_later() {
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
    let journal = GrowingLog {
        inner: MemoryLog::new("engineering"),
        late: AtomicBool::new(true),
    };
    let opened_at = journal
        .inner
        .append("operator", "state the root cause", None, None);
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[("one", vec![vec![], vec![complete("done", None)]])],
    );
    run(run_episode(
        &journal,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        Door {
            chat: "engineering".into(),
            desk_name: "Engineering".into(),
            members: vec!["one".into(), "two".into()],
            starters: vec!["one".into()],
            opened_at,
        },
    ))
    .expect("the episode runs");
    // The late row is above the first wave's watermark: not in that turn,
    // in the next, and in no more than one.
    let prompts = runner.prompts();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(!prompts[0].2.contains("one more thing"), "{}", prompts[0].2);
    assert!(prompts[1].2.contains("one more thing"), "{}", prompts[1].2);
    assert_eq!(runner.since(), vec![None, Some(opened_at)]);
}
