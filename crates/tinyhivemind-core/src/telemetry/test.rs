//! Unit tests for stamping, derivation from fold values, and the wire form.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;
use crate::driver::Event as ConductEvent;
use crate::hive::{HiveStep, Phase, TopicId, Visibility};

fn tracer<'a>(sink: &'a MemorySink, clock: &'a ManualClock) -> Tracer<'a> {
    Tracer::new("r", sink, clock)
}

#[test]
fn stamps_a_gapless_sequence_and_the_clock() {
    let (sink, clock) = (MemorySink::default(), ManualClock::default());
    let tracer = tracer(&sink, &clock);
    clock.set(5);
    tracer.emit(TraceEvent::Idle);
    clock.set(9);
    tracer.emit(TraceEvent::Idle);
    let events = sink.events();
    assert_eq!(
        events.iter().map(|e| (e.seq, e.at_ms)).collect::<Vec<_>>(),
        [(0, 5), (1, 9)]
    );
    assert!(events.iter().all(|e| e.run == "r"));
}

#[test]
fn derives_terminal_steps() {
    let (sink, clock) = (MemorySink::default(), ManualClock::default());
    let tracer = tracer(&sink, &clock);
    tracer.step(&HiveStep::Idle);
    tracer.step(&HiveStep::Deadlocked {
        topics: vec![TopicId("a".into()), TopicId("b".into())],
    });
    tracer.step(&HiveStep::Exhausted {
        spent: 7,
        standings: Vec::new(),
        visibility: Visibility::Blind,
    });
    let kinds: Vec<_> = sink.events().into_iter().map(|e| e.event).collect();
    assert_eq!(kinds[0], TraceEvent::Idle);
    assert_eq!(
        kinds[1],
        TraceEvent::Deadlocked {
            topics: vec![TopicId("a".into()), TopicId("b".into())]
        }
    );
    assert_eq!(
        kinds[2],
        TraceEvent::Exhausted {
            spent: 7,
            visibility: Visibility::Blind,
            advocated: 0
        }
    );
}

#[test]
fn records_conductor_events() {
    let (sink, clock) = (MemorySink::default(), ManualClock::default());
    let tracer = tracer(&sink, &clock);
    let nudged = ConductEvent::Nudged {
        seat: "alice".into(),
        thread: None,
    };
    tracer.conducted(&nudged);
    assert_eq!(
        sink.events()[0].event,
        TraceEvent::Conducted { conducted: nudged }
    );
}

#[test]
fn pins_the_wire_form() {
    let stamped = Stamped {
        run: "r".into(),
        seq: 1,
        at_ms: 20,
        event: TraceEvent::TurnFinished {
            turn: 2,
            seat: "alice".into(),
            input_tokens: 3,
            output_tokens: 4,
            latency_ms: 5,
        },
    };
    let json = serde_json::to_string(&stamped).unwrap();
    assert_eq!(
        json,
        r#"{"run":"r","seq":1,"at_ms":20,"event":"turn_finished","turn":2,"seat":"alice","input_tokens":3,"output_tokens":4,"latency_ms":5}"#
    );
    assert_eq!(serde_json::from_str::<Stamped>(&json).unwrap(), stamped);
    let round = serde_json::to_value(TraceEvent::Round {
        phase: Phase::Commit,
        visibility: Visibility::Full,
        seats: Vec::new(),
    })
    .unwrap();
    assert_eq!(round["event"], "round");
    assert_eq!(round["phase"], "commit");
}

#[test]
fn null_sink_discards_and_debug_names_the_run() {
    let clock = ManualClock::default();
    let tracer = Tracer::new("quiet", &NullSink, &clock);
    tracer.emit(TraceEvent::Idle);
    assert!(format!("{tracer:?}").contains("quiet"));
}

#[test]
fn tool_call_omits_an_absent_refusal_reason() {
    let call = |reason: Option<&str>| TraceEvent::ToolCall {
        turn: 1,
        seat: "a".into(),
        tool: "post".into(),
        latency_ms: 3,
        refused: reason.is_some(),
        reason: reason.map(Into::into),
    };
    let plain = serde_json::to_value(call(None)).unwrap();
    assert!(plain.get("reason").is_none());
    let refused = serde_json::to_value(call(Some("not a member"))).unwrap();
    assert_eq!(refused["reason"], "not a member");
    let back: TraceEvent = serde_json::from_value(plain).unwrap();
    assert_eq!(back, call(None));
}
