//! A journal that keeps every default is briefed as the episode words it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::json;
use tinyhivemind_driver::{BroadcastRouting, CompletionDriver, ConductPolicy, Door};

use super::super::run_episode;
use super::support::{BareJournal, ScriptRunner, complete, hive, policy, run};
use crate::journal::MemoryLog;

#[test]
fn a_journal_that_keeps_the_defaults_is_briefed_as_the_episode_words_it() {
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
    let journal = BareJournal(MemoryLog::new("engineering"));
    let opened_at = journal.0.append("operator", "the task", None, None);
    // The first turn's task panics; the second completes. A panicked task
    // is a failed turn, not a failed wave, and the seat runs again.
    let runner = ScriptRunner::new(
        &["one", "two"],
        &[(
            "one",
            vec![vec![("panic", json!({}))], vec![complete("done", None)]],
        )],
    );
    let report = run(run_episode(
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
    .expect("the episode settles");
    assert_eq!(report.settled, 2);
    assert_eq!(report.conversations, 0);
    // The default composition is the brief alone: the operator's row, as the
    // episode renders it.
    let prompts = runner.prompts();
    assert!(prompts[0].2.starts_with("## "), "{}", prompts[0].2);
    assert!(
        prompts[0].2.contains("@operator: the task"),
        "{}",
        prompts[0].2
    );
    assert_eq!(prompts.len(), 2, "the panicked turn was run again");
    assert!(journal.0.all().iter().any(|row| row.body == "done"));
}
