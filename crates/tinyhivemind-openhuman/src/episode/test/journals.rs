//! A journal that keeps every default is briefed as the episode words it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::json;
use tinyhivemind::{Sequence, SessionLog};
use tinyhivemind_driver::{BroadcastRouting, Commit, CompletionDriver, ConductPolicy, Door, Note};

use super::super::{Journal, run_episode};
use super::support::{BareJournal, ScriptRunner, complete, hive, policy, run};
use crate::Result;
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
    let opened_at = journal.0.append("operator", "the task", None, &[]);
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

#[test]
fn a_journal_that_names_nobody_calls_a_seat_by_its_id() {
    let journal = BareJournal(MemoryLog::new("engineering"));
    assert_eq!(journal.display_name("one"), "one");
}

struct NamedJournal(BareJournal);

impl Journal for NamedJournal {
    fn log(&self) -> &dyn SessionLog {
        self.0.log()
    }

    fn commit(&self, commit: &Commit) -> Result<Sequence> {
        self.0.commit(commit)
    }

    fn note(&self, note: &Note) -> Result<()> {
        self.0.note(note)
    }

    fn display_name(&self, seat: &str) -> String {
        match seat {
            "two" => "Tess".to_owned(),
            other => other.to_owned(),
        }
    }
}

#[test]
fn a_seat_the_host_names_is_written_by_its_name_in_the_brief() {
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
    let journal = NamedJournal(BareJournal(MemoryLog::new("engineering")));
    journal.0.0.append("two", "the port is 8080", None, &[]);
    let opened_at = journal.0.0.append("operator", "the task", None, &[]);
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
        Door {
            chat: "engineering".into(),
            desk_name: "Engineering".into(),
            members: vec!["one".into(), "two".into()],
            starters: vec!["one".into()],
            opened_at,
        },
    ))
    .expect("the episode settles");
    let prompts = runner.prompts();
    let brief = &prompts[0].2;
    assert!(brief.contains("Tess: the port is 8080"), "{brief}");
    assert!(!brief.contains("@two:"), "{brief}");
    assert!(brief.contains("@operator: the task"), "{brief}");
}
