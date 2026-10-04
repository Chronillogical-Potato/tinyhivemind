//! A seat's session persists across activations; only compaction shrinks it.

use std::sync::Mutex;

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::TraceEvent;

use super::support::*;
use crate::swe::context::{Policy, Settings};
use crate::swe::llm::Chat;
use crate::swe::session::{SessionMode, Sessions};
use crate::swe::tools::HIVE_TOOLS;

fn messages(body: &Value) -> Vec<Value> {
    body["messages"].as_array().expect("messages").clone()
}

fn users(body: &Value) -> Vec<String> {
    messages(body)
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| m["content"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn two_activations(rig: &Rig) {
    activate(rig, &Spec::new(HIVE_TOOLS, true, 5));
    activate(
        rig,
        &Spec {
            user: "DELTA: @tester: tests pass".into(),
            ..Spec::new(HIVE_TOOLS, true, 5)
        },
    );
}

fn script() -> Vec<Result<Value, String>> {
    vec![
        Ok(call("bash", json!({ "cmd": "ls" }))),
        Ok(call("post", json!({ "message": "listed" }))),
        Ok(call("post", json!({ "message": "done" }))),
    ]
}

#[test]
fn a_second_activation_still_holds_the_first_ones_tool_results() {
    let (rig, bodies) = recorded(script());
    two_activations(&rig);
    let bodies = bodies.lock().expect("lock");
    let last = bodies.last().expect("a request");
    let all = messages(last);
    assert_eq!(all.iter().filter(|m| m["role"] == "system").count(), 1);
    assert!(
        all.iter()
            .any(|m| m["role"] == "tool" && m["content"].as_str().unwrap_or("").contains("ran ls")),
        "the first activation's command output is still in the session"
    );
    assert_eq!(users(last), ["go", "DELTA: @tester: tests pass"]);
}

#[test]
fn only_the_delta_is_appended_on_resume() {
    let (rig, _) = recorded(script());
    activate(&rig, &Spec::new(HIVE_TOOLS, true, 5));
    let after_first = rig.sessions.len_of("lead");
    activate(
        &rig,
        &Spec {
            user: "DELTA".into(),
            ..Spec::new(HIVE_TOOLS, true, 5)
        },
    );
    // One delta message, then the reply and its tool result.
    assert_eq!(rig.sessions.len_of("lead"), after_first + 3);
}

#[test]
fn a_resumed_session_is_a_typed_event_with_its_size() {
    let (rig, _) = recorded(script());
    two_activations(&rig);
    let resumed: Vec<(String, u32, u32)> = events(&rig)
        .into_iter()
        .filter_map(|e| match e {
            TraceEvent::SessionResumed {
                seat,
                messages,
                delta_rows,
            } => Some((seat, messages, delta_rows)),
            _ => None,
        })
        .collect();
    // Only the second activation resumes; the first opened the session with
    // system, opening, two calls and their results.
    assert_eq!(resumed, [("lead".to_owned(), 6, 0)]);
}

#[test]
fn without_compaction_nothing_is_ever_removed() {
    let (rig, _) = recorded(vec![
        Ok(call("bash", json!({ "cmd": "a" }))),
        Ok(call("post", json!({ "message": "1" }))),
        Ok(call("bash", json!({ "cmd": "b" }))),
        Ok(call("post", json!({ "message": "2" }))),
        Ok(call("bash", json!({ "cmd": "c" }))),
        Ok(call("post", json!({ "message": "3" }))),
    ]);
    let mut sizes = Vec::new();
    for _ in 0..3 {
        activate(&rig, &Spec::new(HIVE_TOOLS, true, 5));
        sizes.push(rig.sessions.len_of("lead"));
    }
    assert_eq!(sizes, [6, 11, 16]);
}

/// Pops the script, except that a summarizer request gets a note.
struct Summarizing(Mutex<Vec<Value>>);

impl Chat for Summarizing {
    fn send(&self, body: &Value) -> Result<Value, String> {
        let system = body["messages"][0]["content"].as_str().unwrap_or_default();
        if system.contains("You condense") {
            return Ok(text("NOTE: summary of earlier work"));
        }
        Ok(self.0.lock().expect("lock").remove(0))
    }
}

#[test]
fn compaction_is_what_shrinks_a_long_session() {
    let mut script = Vec::new();
    for n in 0..3 {
        for _ in 0..3 {
            script.push(long_bash().expect("call"));
        }
        script.push(call("post", json!({ "message": format!("round {n}") })));
    }
    let rig = Rig::over(Box::new(Summarizing(Mutex::new(script))), None);
    let settings = Settings {
        policy: Policy::Summarize,
        budget: 50,
        keep_recent: 1,
    };
    let mut sizes = vec![0];
    for _ in 0..3 {
        activate(
            &rig,
            &Spec {
                context: settings,
                ..Spec::new(HIVE_TOOLS, true, 5)
            },
        );
        sizes.push(rig.sessions.len_of("lead"));
    }
    assert!(context_marks(&rig) >= 1);
    // Each activation adds a user message and four exchanges (9 messages)
    // unless a summary took some out.
    assert!(
        sizes.windows(2).any(|w| w[1] < w[0] + 9),
        "a summary removed messages: {sizes:?}"
    );
    let summary = rig.sessions.take("lead").messages[2]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(summary.contains("NOTE: summary of earlier work"));
}

#[test]
fn fresh_mode_starts_every_activation_from_its_opening_again() {
    let (mut rig, bodies) = recorded(script());
    rig.sessions = Sessions::new(SessionMode::Fresh);
    two_activations(&rig);
    let bodies = bodies.lock().expect("lock");
    let last = bodies.last().expect("a request");
    assert_eq!(messages(last).len(), 2, "system and the new opening only");
    assert_eq!(users(last), ["DELTA: @tester: tests pass"]);
    assert_eq!(rig.sessions.len_of("lead"), 0);
    assert!(
        !events(&rig)
            .iter()
            .any(|e| matches!(e, TraceEvent::SessionResumed { .. })),
        "a fresh session never resumes"
    );
}
