//! Memory recalled at session start, rejoin and compaction; remembered at the
//! end of every activation; failures degrade to no memory.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tinyhivemind_core::runtime::{
    BriefingNote, EntryKind, Error as CoreError, Recall, RecallFuture, RecallMoment,
    RecallRequest, Remember, RememberFuture, RememberRequest,
};
use tinyhivemind_core::telemetry::TraceEvent;

use super::support::*;
use crate::swe::context::{MEMORY_HEADER, Policy, Settings};
use crate::swe::memory::SeatMemory;
use crate::swe::sandbox::{Exec, ExecOutput};
use crate::swe::tools::{HIVE_TOOLS, SINGLE_TOOLS};

#[derive(Default)]
struct Fake {
    recalls: Arc<Mutex<Vec<RecallRequest>>>,
    stored: Arc<Mutex<Vec<RememberRequest>>>,
    broken: bool,
}

impl Recall for Fake {
    fn recall<'a>(&'a self, request: &'a RecallRequest) -> RecallFuture<'a> {
        self.recalls.lock().expect("lock").push(request.clone());
        let broken = self.broken;
        let label = request.moment.label();
        Box::pin(async move {
            if broken {
                return Err(CoreError::Recall {
                    source: "timed out".into(),
                });
            }
            Ok(vec![BriefingNote {
                heading: "@tester".into(),
                lines: vec![format!("PACK-{label}")],
            }])
        })
    }
}

impl Remember for Fake {
    fn remember<'a>(&'a self, request: &'a RememberRequest) -> RememberFuture<'a> {
        self.stored.lock().expect("lock").push(request.clone());
        Box::pin(async { Ok(()) })
    }
}

impl SeatMemory for Fake {
    fn conversation(&self) -> String {
        "team:test".into()
    }

    fn budget_chars(&self) -> usize {
        2000
    }
}

type Log<T> = Arc<Mutex<Vec<T>>>;

fn with_memory(rig: &mut Rig, broken: bool) -> (Log<RecallRequest>, Log<RememberRequest>) {
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

fn recalled(rig: &Rig) -> Vec<(String, u32, u64)> {
    events(rig)
        .into_iter()
        .filter_map(|e| match e {
            TraceEvent::Recalled {
                moment,
                notes,
                chars,
                ..
            } => Some((moment, notes, chars)),
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
    let kinds: Vec<EntryKind> = stored[0].entries.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        [EntryKind::FailedAttempt, EntryKind::FailedAttempt, EntryKind::Outcome]
    );
    assert!(stored[0].entries[0].text.contains("`pytest -x` (failed, exit 1) -> ModuleNotFoundError"));
    assert!(stored[0].entries[1].text.contains("did not run"));
    assert_eq!(stored[0].entries[2].text, "pytest fails on foo");
    assert!(events(&rig).iter().any(|e| matches!(e, TraceEvent::Remembered { entries: 3, .. })));
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
    let Some(RecallMoment::Compaction { dropped }) = recalls.get(1).map(|r| r.moment.clone())
    else {
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
            .any(|m| m.contains("session_start") && m.contains("error=memory recall failed: timed out"))
    );
    assert_eq!(recalled(&rig), [("session_start".to_owned(), 0, 0)]);
}
