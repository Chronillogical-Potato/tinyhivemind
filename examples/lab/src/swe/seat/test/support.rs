//! Shared fixtures: a scripted model, a recording model, a fake sandbox and
//! the rig that runs one activation on a persistent session.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::{Clock, Stamped, TraceEvent, TraceSink, Tracer};

use super::super::*;
use crate::swe::context::{Policy, Settings};
use crate::swe::llm::Chat;
use crate::swe::memory::SeatMemory;
use crate::swe::meter::Meter;
use crate::swe::sandbox::{Exec, ExecOutput};
use crate::swe::session::{SessionMode, Sessions};
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
    pub(super) llm: Llm,
    pub(super) board: Board,
    pub(super) sink: Events,
    pub(super) turns: AtomicU64,
    pub(super) sessions: Sessions,
    pub(super) memory: Option<Box<dyn SeatMemory>>,
}

impl Rig {
    pub(super) fn over(chat: Box<dyn Chat>, cap: Option<u64>) -> Self {
        Self {
            llm: Llm::new(chat, "m", Meter::new(cap, None)).without_retry_pause(),
            board: Board::new(&["lead", "tester"], 6),
            sink: Events(Mutex::new(Vec::new())),
            turns: AtomicU64::new(0),
            sessions: Sessions::new(SessionMode::Persistent),
            memory: None,
        }
    }
}

pub(super) fn rig(script: Vec<Value>, cap: Option<u64>) -> Rig {
    Rig::over(Box::new(Script(Mutex::new(script))), cap)
}

/// What one test activation runs with; `Spec::new` is the old `go` call.
pub(super) struct Spec {
    pub(super) speaking: &'static [&'static str],
    pub(super) implicit: bool,
    pub(super) steps: usize,
    pub(super) context: Settings,
    pub(super) user: String,
}

impl Spec {
    pub(super) fn new(speaking: &'static [&'static str], implicit: bool, steps: usize) -> Self {
        Self {
            speaking,
            implicit,
            steps,
            context: Settings::OFF,
            user: "go".into(),
        }
    }
}

pub(super) fn go(
    rig: &Rig,
    speaking: &'static [&'static str],
    implicit: bool,
    steps: usize,
) -> Outcome {
    activate(rig, &Spec::new(speaking, implicit, steps))
}

pub(super) fn go_with(
    rig: &Rig,
    speaking: &'static [&'static str],
    implicit: bool,
    steps: usize,
    context: Settings,
) -> Outcome {
    activate(
        rig,
        &Spec {
            context,
            ..Spec::new(speaking, implicit, steps)
        },
    )
}

/// Run one activation of `lead` on its session from the rig's store.
pub(super) fn activate(rig: &Rig, spec: &Spec) -> Outcome {
    activate_on(rig, &Echo, spec)
}

/// [`activate`] with commands running on `exec`.
pub(super) fn activate_on(rig: &Rig, exec: &dyn Exec, spec: &Spec) -> Outcome {
    let tracer = Tracer::new("t", &rig.sink, &Zero);
    let env = Env {
        llm: &rig.llm,
        exec,
        tracer: &tracer,
        board: &rig.board,
        turns: &rig.turns,
        cmd_timeout: Duration::from_secs(1),
        output_limit: 1000,
        sessions: &rig.sessions,
        memory: rig.memory.as_deref(),
    };
    let mut session = rig.sessions.take("lead");
    let out = run(
        &env,
        &Activation {
            seat: "lead",
            system: "sys".into(),
            user: spec.user.clone(),
            shown_rows: 0,
            focus: "the task".into(),
            tools: tool_list(spec.speaking),
            speaking: spec.speaking,
            steps: spec.steps,
            implicit_post: spec.implicit,
            context: spec.context,
        },
        &mut session,
    );
    rig.sessions.put("lead", session);
    out
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
    pub(super) script: Mutex<Vec<Result<Value, String>>>,
    pub(super) bodies: Arc<Mutex<Vec<Value>>>,
}

impl Chat for Recorder {
    fn send(&self, body: &Value) -> Result<Value, String> {
        self.bodies.lock().expect("lock").push(body.clone());
        self.script.lock().expect("lock").remove(0)
    }
}

pub(super) fn recorded(script: Vec<Result<Value, String>>) -> (Rig, Arc<Mutex<Vec<Value>>>) {
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let rig = Rig::over(
        Box::new(Recorder {
            script: Mutex::new(script),
            bodies: bodies.clone(),
        }),
        None,
    );
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
