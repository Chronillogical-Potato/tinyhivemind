//! Memory recalled at session start, rejoin and compaction; remembered at the
//! end of every activation; failures degrade to no memory.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::TraceEvent;

use super::support::*;
use crate::swe::context::{MEMORY_HEADER, Policy, Settings};
use crate::swe::memory::{Moment, Recalled, Remembered, Report, SeatMemory};
use crate::swe::sandbox::{Exec, ExecOutput};
use crate::swe::tools::{HIVE_TOOLS, SINGLE_TOOLS};

#[derive(Default)]
struct Fake {
    recalls: Arc<Mutex<Vec<Moment>>>,
    stored: Arc<Mutex<Vec<Remembered>>>,
    broken: bool,
}

impl SeatMemory for Fake {
    fn recall(&self, _seat: &str, moment: &Moment) -> Recalled {
        self.recalls.lock().expect("lock").push(moment.clone());
        if self.broken {
            return Recalled {
                pack: None,
                report: Report {
                    op: "recall",
                    moment: moment.name(),
                    error: Some("timed out".into()),
                    ..Report::default()
                },
            };
        }
        Recalled {
            pack: Some(format!("{MEMORY_HEADER}\nPACK-{}", moment.name())),
            report: Report {
                op: "recall",
                moment: moment.name(),
                chars: 10,
                ..Report::default()
            },
        }
    }

    fn remember(&self, _seat: &str, what: &Remembered) -> Report {
        self.stored.lock().expect("lock").push(what.clone());
        Report {
            op: "remember",
            ..Report::default()
        }
    }
}

type Log<T> = Arc<Mutex<Vec<T>>>;

fn with_memory(rig: &mut Rig, broken: bool) -> (Log<Moment>, Log<Remembered>) {
    let fake = Fake {
        broken,
        ..Fake::default()
    };
    let logs = (fake.recalls.clone(), fake.stored.clone());
    rig.memory = Some(Box::new(fake));
    logs
}

fn user_texts(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| m["content"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn memory_marks(rig: &Rig) -> Vec<String> {
    events(rig)
        .into_iter()
        .filter_map(|e| match e {
            TraceEvent::Mark { label, detail } if label == "memory" => Some(detail),
            _ => None,
        })
        .collect()
}

#[test]
fn a_new_session_opens_with_the_pack_and_a_resumed_one_appends_it() {
    let (mut rig, bodies) = recorded(vec![
        Ok(call("post", json!({ "message": "one" }))),
        Ok(call("post", json!({ "message": "two" }))),
    ]);
    let (recalls, _) = with_memory(&mut rig, false);
    activate(&rig, &Spec::new(HIVE_TOOLS, true, 3));
    activate(
        &rig,
        &Spec {
            user: "DELTA".into(),
            ..Spec::new(HIVE_TOOLS, true, 3)
        },
    );
    let bodies = bodies.lock().expect("lock");
    let users = user_texts(bodies.last().expect("request"));
    assert!(users[0].starts_with(MEMORY_HEADER) && users[0].ends_with("go"));
    assert!(users[1].starts_with("DELTA") && users[1].contains("PACK-rejoin"));
    let names: Vec<&str> = recalls
        .lock()
        .expect("lock")
        .iter()
        .map(Moment::name)
        .collect();
    assert_eq!(names, ["session_start", "rejoin"]);
    assert!(
        memory_marks(&rig)
            .iter()
            .any(|m| m.contains("recall rejoin"))
    );
}

struct Failing;
impl Exec for Failing {
    fn exec(&self, cmd: &str, _t: std::time::Duration) -> Result<ExecOutput, String> {
        Ok(ExecOutput {
            stdout: format!("running {cmd}\nModuleNotFoundError: no module named foo"),
            exit: i32::from(cmd.contains("pytest")),
        })
    }
}

#[test]
fn the_end_of_an_activation_stores_its_words_and_a_ledger_of_attempts() {
    let (mut rig, _) = recorded(vec![
        Ok(call("bash", json!({ "cmd": "pytest -x" }))),
        Ok(call("bash", json!({ "cmd": "rm -rf /" }))),
        Ok(call("post", json!({ "message": "pytest fails on foo" }))),
    ]);
    let (_, stored) = with_memory(&mut rig, false);
    let out = activate_on(&rig, &Failing, &Spec::new(HIVE_TOOLS, true, 5));
    assert_eq!(out.ledger.len(), 2);
    let stored = stored.lock().expect("lock");
    assert_eq!(stored.len(), 1);
    let turn = stored[0].render();
    assert!(turn.starts_with("pytest fails on foo"));
    assert!(turn.contains("[FAILED attempt, exit 1] `pytest -x` -> ModuleNotFoundError"));
    assert!(turn.contains("[FAILED attempt, did not run] `rm -rf /`"));
}

#[test]
fn a_summary_carries_a_compaction_recall_and_flushes_the_ledger_first() {
    let (mut rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        Ok(text("NOTE: wrote the file")),
        long_bash(),
        Ok(text("NOTE: wrote the file")),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let (recalls, stored) = with_memory(&mut rig, false);
    let settings = Settings {
        policy: Policy::Summarize,
        budget: 50,
        keep_recent: 1,
    };
    go_with(&rig, SINGLE_TOOLS, false, 10, settings);
    let recalls = recalls.lock().expect("lock");
    let Some(Moment::Compaction { dropped, .. }) = recalls.get(1) else {
        panic!("second recall is a compaction: {recalls:?}");
    };
    assert!(!dropped.is_empty());
    assert!(
        stored.lock().expect("lock").len() >= 2,
        "flushed, then the end"
    );
    let bodies = bodies.lock().expect("lock");
    let users = user_texts(bodies.last().expect("request"));
    assert!(
        users[1].contains("PACK-compaction"),
        "pack after the opening: {users:?}"
    );
}

#[test]
fn a_failing_memory_is_reported_and_the_activation_carries_on() {
    let (mut rig, bodies) = recorded(vec![Ok(call("post", json!({ "message": "ok" })))]);
    with_memory(&mut rig, true);
    let out = activate(&rig, &Spec::new(HIVE_TOOLS, true, 3));
    assert!(out.spoke.is_some());
    let bodies = bodies.lock().expect("lock");
    assert_eq!(user_texts(&bodies[0]), ["go"]);
    assert!(
        memory_marks(&rig)
            .iter()
            .any(|m| m.contains("session_start") && m.contains("error=timed out"))
    );
}
