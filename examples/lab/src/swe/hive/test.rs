//! Queue discipline, routing and a whole scripted hive episode.

use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use serde_json::{Value, json};
use tinyhivemind_core::telemetry::{Clock, Stamped, TraceEvent, TraceSink, Tracer};

use super::*;
use crate::swe::board::Board;
use crate::swe::llm::{Chat, Llm};
use crate::swe::meter::Meter;
use crate::swe::sandbox::{Exec, ExecOutput};

fn wake(seat: &str) -> Wake {
    Wake {
        seat: seat.into(),
        reason: BidReason::Addressed,
        note: String::new(),
    }
}

#[test]
fn enqueue_dedupes_and_rounds_are_width_bounded() {
    let mut queue = Vec::new();
    for seat in ["a", "b", "a", "c"] {
        enqueue(&mut queue, wake(seat));
    }
    assert_eq!(queue.len(), 3);
    let round = take_round(&mut queue, 2);
    assert_eq!(round.len(), 2);
    assert_eq!(queue.len(), 1);
    assert_eq!(take_round(&mut queue, 0).len(), 1, "width floors at one");
}

#[test]
fn broadcasts_route_by_keyword_and_never_to_the_author_or_lead() {
    assert_eq!(
        route_broadcast("lead", "please run the tests and verify"),
        "tester"
    );
    assert_eq!(
        route_broadcast("lead", "review the diff for regressions"),
        "reviewer"
    );
    assert_eq!(route_broadcast("lead", "fix the parser"), "implementer");
    assert_eq!(route_broadcast("lead", "???"), "implementer");
    assert_eq!(
        route_broadcast("implementer", "fix and implement"),
        "tester"
    );
}

/// A model that answers by the seat named in the system prompt's role line.
struct Roles(Mutex<Vec<String>>);

impl Chat for Roles {
    fn send(&self, body: &Value) -> Result<Value, String> {
        let system = body["messages"][0]["content"].as_str().unwrap_or_default();
        let last = body["messages"]
            .as_array()
            .and_then(|m| m.last())
            .cloned()
            .unwrap_or_default();
        let tool_turn = last["role"] == "tool";
        let (name, args) = if system.contains("You are the lead") {
            let user = body["messages"][1]["content"].as_str().unwrap_or_default();
            if user.contains("reported") {
                ("complete_episode", json!({ "message": "all done" }))
            } else {
                (
                    "broadcast",
                    json!({ "message": "fix the bug in parser.py" }),
                )
            }
        } else if tool_turn {
            ("post", json!({ "message": "fixed parser.py" }))
        } else {
            ("bash", json!({ "cmd": "echo fix" }))
        };
        if let Ok(mut seen) = self.0.lock() {
            seen.push(name.to_owned());
        }
        Ok(json!({
            "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
                { "id": "c", "type": "function", "function": { "name": name, "arguments": args.to_string() } }] } }],
            "usage": { "prompt_tokens": 40, "completion_tokens": 4 }
        }))
    }
}

struct Echo;
impl Exec for Echo {
    fn exec(&self, _c: &str, _t: Duration) -> Result<ExecOutput, String> {
        Ok(ExecOutput {
            stdout: "ok".into(),
            exit: 0,
        })
    }
}

struct Events(Mutex<Vec<Stamped>>);
impl TraceSink for Events {
    fn record(&self, e: &Stamped) {
        self.0.lock().expect("lock").push(e.clone());
    }
}
struct Zero;
impl Clock for Zero {
    fn now_ms(&self) -> u64 {
        0
    }
}

#[test]
fn a_scripted_episode_converges_when_the_lead_completes() {
    let sink = Events(Mutex::new(Vec::new()));
    let tracer = Tracer::new("h", &sink, &Zero);
    let llm = Llm::new(
        Box::new(Roles(Mutex::new(Vec::new()))),
        "m",
        Meter::new(None, Some(40)),
    );
    let board = Board::new(&["lead", "implementer", "tester", "reviewer"], 6);
    let turns = AtomicU64::new(0);
    let env = Env {
        llm: &llm,
        exec: &Echo,
        tracer: &tracer,
        board: &board,
        turns: &turns,
        cmd_timeout: Duration::from_secs(1),
        output_limit: 500,
    };
    let report = run(
        &env,
        "fix parser",
        &Params {
            round_width: 2,
            steps: 4,
        },
    );
    assert!(report.completed, "report: {report:?}");
    assert!(report.abort.is_none());
    assert!(report.rounds >= 3);
    let events: Vec<TraceEvent> = sink
        .0
        .lock()
        .expect("lock")
        .iter()
        .map(|s| s.event.clone())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TraceEvent::Converged { .. }))
    );
    let widest = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Round { seats, .. } => Some(seats.len()),
            _ => None,
        })
        .max();
    assert!(widest <= Some(2));
    assert!(board.read("lead", 10).contains("fixed parser.py"));
}

#[test]
fn the_meter_cap_stops_the_hive_as_exhausted() {
    let sink = Events(Mutex::new(Vec::new()));
    let tracer = Tracer::new("h", &sink, &Zero);
    let llm = Llm::new(
        Box::new(Roles(Mutex::new(Vec::new()))),
        "m",
        Meter::new(None, Some(1)),
    );
    let board = Board::new(&["lead", "implementer", "tester", "reviewer"], 6);
    let turns = AtomicU64::new(0);
    let env = Env {
        llm: &llm,
        exec: &Echo,
        tracer: &tracer,
        board: &board,
        turns: &turns,
        cmd_timeout: Duration::from_secs(1),
        output_limit: 500,
    };
    let report = run(
        &env,
        "t",
        &Params {
            round_width: 2,
            steps: 4,
        },
    );
    assert!(!report.completed);
    assert!(report.abort.is_some());
    let events: Vec<TraceEvent> = sink
        .0
        .lock()
        .expect("lock")
        .iter()
        .map(|s| s.event.clone())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TraceEvent::Exhausted { .. }))
    );
}
