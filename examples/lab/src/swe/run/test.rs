//! Both arms through `run`, with the same scripted model and sandbox.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::{Clock, Stamped, TraceSink, Tracer};

use super::*;
use crate::swe::config::Target;
use crate::swe::llm::Chat;
use crate::swe::meter::Meter;
use crate::swe::sandbox::{Exec, ExecOutput};

struct Finisher;

impl Chat for Finisher {
    fn send(&self, _body: &Value) -> Result<Value, String> {
        Ok(json!({
            "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
                { "id": "c", "type": "function",
                  "function": { "name": "complete_episode", "arguments": "{\"message\":\"done\"}" } }] } }],
            "usage": { "prompt_tokens": 30, "completion_tokens": 3 }
        }))
    }
}

struct Nop;
impl Exec for Nop {
    fn exec(&self, _c: &str, _t: Duration) -> Result<ExecOutput, String> {
        Ok(ExecOutput::default())
    }
}

struct Sink(Mutex<usize>);
impl TraceSink for Sink {
    fn record(&self, _e: &Stamped) {
        *self.0.lock().expect("lock") += 1;
    }
}
struct Zero;
impl Clock for Zero {
    fn now_ms(&self) -> u64 {
        0
    }
}

fn config(mode: Mode) -> Config {
    Config::parse(["--mode", mode.name(), "--task", "t", "--container", "c"].map(str::to_owned))
        .map(|mut c| {
            c.target = Target::StdioRpc;
            c
        })
        .expect("config")
}

fn summary(mode: Mode) -> (Summary, usize) {
    let sink = Sink(Mutex::new(0));
    let tracer = Tracer::new("r", &sink, &Zero);
    let llm = Llm::new(Box::new(Finisher), "m", Meter::new(None, None));
    let s = run(&config(mode), &llm, &Nop, &tracer);
    let events = *sink.0.lock().expect("lock");
    (s, events)
}

#[test]
fn single_completes_in_one_call_and_reports_tokens() {
    let (s, events) = summary(Mode::Single);
    assert!(s.completed && s.aborted.is_none());
    assert_eq!(
        (s.mode, s.turns, s.tokens_in, s.tokens_out),
        ("single", 1, 30, 3)
    );
    assert!(events >= 5);
}

#[test]
fn hive_completes_when_the_lead_completes() {
    let (s, _) = summary(Mode::Hive);
    assert!(s.completed);
    assert_eq!((s.mode, s.rounds, s.activations), ("hive", 1, 1));
    let doc = s.to_json();
    for key in [
        "mode",
        "model",
        "tokens_in",
        "tokens_out",
        "wall_ms",
        "turns",
        "completed",
    ] {
        assert!(doc.get(key).is_some(), "result.json lacks {key}");
    }
    assert_eq!(doc["seats"]["lead"]["calls"], 1);
}
