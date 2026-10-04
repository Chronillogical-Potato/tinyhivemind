//! Activations against a scripted model and a fake sandbox.

use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::{Clock, Stamped, TraceEvent, TraceSink, Tracer};

use super::*;
use crate::swe::context::{Policy, Settings};
use crate::swe::llm::Chat;
use crate::swe::meter::Meter;
use crate::swe::sandbox::{Exec, ExecOutput};
use crate::swe::tools::{HIVE_TOOLS, SINGLE_TOOLS, tool_list};

struct Script(Mutex<Vec<Value>>);

impl Chat for Script {
    fn send(&self, _body: &Value) -> Result<Value, String> {
        let mut turns = self.0.lock().expect("lock");
        if turns.is_empty() {
            return Err("script exhausted".into());
        }
        Ok(turns.remove(0))
    }
}

fn call(name: &str, args: Value) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
            { "id": "c", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
        ] } }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 10 }
    })
}

fn text(content: &str) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 50, "completion_tokens": 5 }
    })
}

struct Echo;

impl Exec for Echo {
    fn exec(&self, cmd: &str, _t: Duration) -> Result<ExecOutput, String> {
        Ok(ExecOutput {
            stdout: format!("ran {cmd}"),
            exit: 0,
        })
    }
}

struct Events(Mutex<Vec<Stamped>>);

impl TraceSink for Events {
    fn record(&self, event: &Stamped) {
        self.0.lock().expect("lock").push(event.clone());
    }
}

struct Zero;

impl Clock for Zero {
    fn now_ms(&self) -> u64 {
        0
    }
}

struct Rig {
    llm: Llm,
    board: Board,
    sink: Events,
    turns: AtomicU64,
}

fn rig(script: Vec<Value>, cap: Option<u64>) -> Rig {
    Rig {
        llm: Llm::new(
            Box::new(Script(Mutex::new(script))),
            "m",
            Meter::new(cap, None),
        )
        .without_retry_pause(),
        board: Board::new(&["lead", "tester"], 6),
        sink: Events(Mutex::new(Vec::new())),
        turns: AtomicU64::new(0),
    }
}

fn go(rig: &Rig, speaking: &'static [&'static str], implicit: bool, steps: usize) -> Outcome {
    go_with(rig, speaking, implicit, steps, Settings::OFF)
}

fn go_with(
    rig: &Rig,
    speaking: &'static [&'static str],
    implicit: bool,
    steps: usize,
    context: Settings,
) -> Outcome {
    let tracer = Tracer::new("t", &rig.sink, &Zero);
    let env = Env {
        llm: &rig.llm,
        exec: &Echo,
        tracer: &tracer,
        board: &rig.board,
        turns: &rig.turns,
        cmd_timeout: Duration::from_secs(1),
        output_limit: 1000,
    };
    run(
        &env,
        &Activation {
            seat: "lead",
            system: "sys".into(),
            user: "go".into(),
            tools: tool_list(speaking),
            speaking,
            steps,
            implicit_post: implicit,
            context,
        },
    )
}

fn events(rig: &Rig) -> Vec<TraceEvent> {
    rig.sink
        .0
        .lock()
        .expect("lock")
        .iter()
        .map(|s| s.event.clone())
        .collect()
}

#[test]
fn bash_then_post_ends_the_activation_with_real_tokens_traced() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "ls" })),
            call("post", json!({ "message": "listed" })),
        ],
        None,
    );
    let out = go(&rig, HIVE_TOOLS, true, 5);
    assert!(out.spoke.is_some() && !out.completed);
    assert_eq!(rig.board.len(), 1);
    let ev = events(&rig);
    assert!(
        ev.iter()
            .any(|e| matches!(e, TraceEvent::Mark { label, .. } if label == "exec"))
    );
    let finished: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            TraceEvent::TurnFinished {
                input_tokens,
                output_tokens,
                ..
            } => Some((*input_tokens, *output_tokens)),
            _ => None,
        })
        .collect();
    assert_eq!(finished, [(100, 10), (100, 10)]);
}

#[test]
fn complete_episode_marks_completion() {
    let rig = rig(
        vec![call("complete_episode", json!({ "message": "done" }))],
        None,
    );
    let out = go(&rig, SINGLE_TOOLS, false, 5);
    assert!(out.completed);
}

#[test]
fn blocked_bash_is_traced_as_refused_with_a_reason() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "rm -rf /" })),
            call("complete_episode", json!({ "message": "x" })),
        ],
        None,
    );
    go(&rig, SINGLE_TOOLS, false, 5);
    let refused = events(&rig).into_iter().find_map(|e| match e {
        TraceEvent::ToolCall {
            tool,
            refused: true,
            reason,
            ..
        } if tool == "bash" => reason,
        _ => None,
    });
    assert!(refused.expect("refused bash").contains("root"));
}

#[test]
fn unoffered_and_malformed_speech_is_refused_not_committed() {
    let rig = rig(
        vec![
            call("dm", json!({ "to": ["tester"], "message": "psst" })),
            call("post", json!({ "message": "  " })),
            call("post", json!({ "message": "ok" })),
        ],
        None,
    );
    let out = go(&rig, HIVE_TOOLS, true, 5);
    assert_eq!(rig.board.len(), 1);
    assert!(out.spoke.is_some());
    let refusals = events(&rig)
        .iter()
        .filter(|e| matches!(e, TraceEvent::ToolCall { refused: true, .. }))
        .count();
    assert_eq!(refusals, 2);
}

#[test]
fn hive_text_without_a_tool_becomes_a_post() {
    let rig = rig(vec![text("I looked and found nothing")], None);
    let out = go(&rig, HIVE_TOOLS, true, 5);
    assert!(out.spoke.is_some());
    assert!(rig.board.read("lead", 5).contains("found nothing"));
}

#[test]
fn single_text_without_a_tool_is_nudged_then_given_up() {
    let rig = rig(vec![text("a"), text("b"), text("c")], None);
    let out = go(&rig, SINGLE_TOOLS, false, 9);
    assert!(out.spoke.is_none() && !out.completed);
    assert_eq!(out.steps, 3);
}

#[test]
fn token_cap_aborts_cleanly_mid_activation() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "ls" })),
            call("bash", json!({ "cmd": "ls" })),
        ],
        Some(100),
    );
    let out = go(&rig, SINGLE_TOOLS, false, 9);
    assert!(matches!(out.abort, Some(Abort::TokenCap { .. })));
    assert_eq!(out.steps, 1);
}

#[test]
fn running_out_of_steps_posts_a_stop_note_in_hive_mode() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "a" })),
            call("bash", json!({ "cmd": "b" })),
        ],
        None,
    );
    let out = go(&rig, HIVE_TOOLS, true, 2);
    assert!(out.spoke.is_some());
    assert!(rig.board.read("lead", 5).contains("stopped after 2 steps"));
}

/// Records every request body, then answers from a script.
struct Recorder {
    script: Mutex<Vec<Result<Value, String>>>,
    bodies: std::sync::Arc<Mutex<Vec<Value>>>,
}

impl Chat for Recorder {
    fn send(&self, body: &Value) -> Result<Value, String> {
        self.bodies.lock().expect("lock").push(body.clone());
        self.script.lock().expect("lock").remove(0)
    }
}

fn recorded(script: Vec<Result<Value, String>>) -> (Rig, std::sync::Arc<Mutex<Vec<Value>>>) {
    let bodies = std::sync::Arc::new(Mutex::new(Vec::new()));
    let rig = Rig {
        llm: Llm::new(
            Box::new(Recorder {
                script: Mutex::new(script),
                bodies: bodies.clone(),
            }),
            "m",
            Meter::new(None, None),
        )
        .without_retry_pause(),
        board: Board::new(&["lead", "tester"], 6),
        sink: Events(Mutex::new(Vec::new())),
        turns: AtomicU64::new(0),
    };
    (rig, bodies)
}

fn long_bash() -> Result<Value, String> {
    Ok(call(
        "bash",
        json!({ "cmd": format!("echo {}", "a".repeat(200)) }),
    ))
}

fn settings(policy: Policy) -> Settings {
    Settings {
        policy,
        budget: 50,
        keep_recent: 1,
    }
}

fn context_marks(rig: &Rig) -> usize {
    events(rig)
        .iter()
        .filter(|e| matches!(e, TraceEvent::Mark { label, .. } if label == "context"))
        .count()
}

#[test]
fn mask_stubs_old_results_once_the_prompt_passes_the_budget() {
    let (rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        long_bash(),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let out = go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::Mask));
    assert!(out.completed);
    assert_eq!(out.max_prompt, 100);
    assert_eq!(rig.llm.meter().snapshot().max_prompt, 100);
    assert!(context_marks(&rig) >= 1);
    let last = bodies.lock().expect("lock").pop().expect("a request");
    let tools: Vec<&str> = last["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().expect("text"))
        .collect();
    assert_eq!(tools.len(), 3);
    assert!(tools[0].starts_with("[output elided:") && tools[1].starts_with("[output elided:"));
    assert!(
        tools[2].starts_with("exit=0"),
        "newest stays whole: {}",
        tools[2]
    );
}

#[test]
fn none_never_rewrites_the_conversation() {
    let (rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::None));
    assert_eq!(context_marks(&rig), 0);
    let last = bodies.lock().expect("lock").pop().expect("a request");
    assert!(!last.to_string().contains("output elided"));
}

#[test]
fn summarize_spends_one_metered_call_and_replaces_the_oldest_messages() {
    let (rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        Ok(text("NOTE: wrote the file")),
        long_bash(),
        Ok(text("NOTE: wrote the file")),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let out = go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::Summarize));
    assert!(out.completed);
    let snap = rig.llm.meter().snapshot();
    assert_eq!((snap.calls, snap.context_events), (6, 2));
    assert_eq!(context_marks(&rig), 2);
    let last = bodies.lock().expect("lock").pop().expect("a request");
    let messages = last["messages"].as_array().expect("messages");
    assert_eq!(messages[1]["content"], "go");
    assert!(
        messages[2]["content"]
            .as_str()
            .expect("text")
            .contains("NOTE: wrote the file")
    );
    let calls = messages
        .iter()
        .filter(|m| m["tool_calls"].is_array())
        .count();
    let results = messages.iter().filter(|m| m["role"] == "tool").count();
    assert_eq!(calls, results);
}

#[test]
fn a_failed_summary_leaves_the_conversation_alone() {
    let (rig, _) = recorded(vec![
        long_bash(),
        long_bash(),
        Err("boom".into()),
        Err("boom".into()),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let out = go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::Summarize));
    assert!(out.abort.is_some() || out.completed);
    assert_eq!(context_marks(&rig), 0);
}

#[test]
fn a_context_overflow_error_aborts_without_a_retry() {
    let (rig, bodies) = recorded(vec![Err(
        "http 400: This model's maximum context length is 131072 tokens".into(),
    )]);
    let out = go(&rig, SINGLE_TOOLS, false, 5);
    assert!(matches!(out.abort, Some(Abort::ContextOverflow(_))));
    assert_eq!(bodies.lock().expect("lock").len(), 1);
    assert_eq!(out.abort.expect("abort").to_string(), "context_overflow");
}
