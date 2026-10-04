//! Shared fixtures: a scripted model, a recording model, a fake sandbox and
//! the rig that runs one activation on a persistent session.

use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::{Clock, Stamped, TraceEvent, TraceSink, Tracer};

use super::super::*;
use crate::swe::memory::SeatMemory;
use crate::swe::session::{SessionMode, Sessions};
use crate::swe::context::{Policy, Settings};
use crate::swe::llm::Chat;
use crate::swe::meter::Meter;
use crate::swe::sandbox::{Exec, ExecOutput};
use crate::swe::tools::tool_list;

pub(super) struct Script(Mutex<Vec<Value>>);

impl Chat for Script {
    fn send(&self, _body: &Value) -> Result<Value, String> {
        let mut turns = self.0.lock().expect("lock");
        if turns.is_empty() {
            return Err("script exhausted".into());
        }
        Ok(turns.remove(0))
    }
}

pub(super) fn call(name: &str, args: Value) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
            { "id": "c", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
        ] } }],
        "usage": { "prompt_tokens": 100, "completion_tokens": 10 }
    })
}

pub(super) fn text(content: &str) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 50, "completion_tokens": 5 }
    })
}

pub(super) struct Echo;

impl Exec for Echo {
    fn exec(&self, cmd: &str, _t: Duration) -> Result<ExecOutput, String> {
        Ok(ExecOutput {
            stdout: format!("ran {cmd}"),
            exit: 0,
        })
    }
}

pub(super) struct Events(Mutex<Vec<Stamped>>);

impl TraceSink for Events {
    fn record(&self, event: &Stamped) {
        self.0.lock().expect("lock").push(event.clone());
    }
}

pub(super) struct Zero;

impl Clock for Zero {
    fn now_ms(&self) -> u64 {
        0
    }
}

pub(super) struct Rig {
    llm: Llm,
    board: Board,
    sink: Events,
    turns: AtomicU64,
}

pub(super) fn rig(script: Vec<Value>, cap: Option<u64>) -> Rig {
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

pub(super) fn go(rig: &Rig, speaking: &'static [&'static str], implicit: bool, steps: usize) -> Outcome {
    go_with(rig, speaking, implicit, steps, Settings::OFF)
}

pub(super) fn go_with(
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

pub(super) fn events(rig: &Rig) -> Vec<TraceEvent> {
    rig.sink
        .0
        .lock()
        .expect("lock")
        .iter()
        .map(|s| s.event.clone())
        .collect()
}


/// Records every request body, then answers from a script.
pub(super) struct Recorder {
    script: Mutex<Vec<Result<Value, String>>>,
    bodies: std::sync::Arc<Mutex<Vec<Value>>>,
}

impl Chat for Recorder {
    fn send(&self, body: &Value) -> Result<Value, String> {
        self.bodies.lock().expect("lock").push(body.clone());
        self.script.lock().expect("lock").remove(0)
    }
}

pub(super) fn recorded(script: Vec<Result<Value, String>>) -> (Rig, std::sync::Arc<Mutex<Vec<Value>>>) {
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

pub(super) fn long_bash() -> Result<Value, String> {
    Ok(call(
        "bash",
        json!({ "cmd": format!("echo {}", "a".repeat(200)) }),
    ))
}

pub(super) fn settings(policy: Policy) -> Settings {
    Settings {
        policy,
        budget: 50,
        keep_recent: 1,
    }
}

pub(super) fn context_marks(rig: &Rig) -> usize {
    events(rig)
        .iter()
        .filter(|e| matches!(e, TraceEvent::Mark { label, .. } if label == "context"))
        .count()
}