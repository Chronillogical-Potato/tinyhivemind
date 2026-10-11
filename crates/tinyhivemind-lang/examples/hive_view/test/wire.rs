//! The pinned NDJSON wire form: every frame the viewer reads.

use serde_json::json;

use crate::Result;
use crate::compare::BASELINE;
use crate::frames::{
    CompareFrame, Frame, ProblemFrame, ReferralFrame, SeedResult, StandingFrame, SummaryRow,
    TurnFrame, UsageFrame,
};

#[test]
fn frames_pin_their_wire_form() -> Result<()> {
    let start = Frame::Start {
        package: "math-panel".into(),
        seed: 3,
        seats: 5,
        problem: Some(ProblemFrame {
            id: "lattice-6x6".into(),
            statement: "s".into(),
            answer: "924".into(),
        }),
        live: None,
        model: None,
    };
    assert_eq!(
        serde_json::to_value(&start)?,
        json!({"type":"start","package":"math-panel","seed":3,"seats":5,
               "problem":{"id":"lattice-6x6","statement":"s","answer":"924"}})
    );
    let round = Frame::Round {
        round: 1,
        desk: "panel".into(),
        desk_round: 1,
        phase: "deliberate".into(),
        visibility: "blind".into(),
        turns: vec![TurnFrame {
            seat: "checker".into(),
            desk: "panel".into(),
            reason: "salience".into(),
            saw: 1,
            of: 4,
            content: "!propose #924 ^1".into(),
            answer: Some("924".into()),
            correct: Some(true),
            sequence: 5,
        }],
        standings: vec![StandingFrame {
            topic: "924".into(),
            support: 2,
            object: 1,
            committed: false,
            supporters: vec!["checker".into(), "lead".into()],
        }],
    };
    assert_eq!(
        serde_json::to_value(&round)?,
        json!({"type":"round","round":1,"desk":"panel","desk_round":1,"phase":"deliberate","visibility":"blind",
               "turns":[{"seat":"checker","desk":"panel","reason":"salience","saw":1,"of":4,
                         "content":"!propose #924 ^1","answer":"924","correct":true,"sequence":5}],
               "standings":[{"topic":"924","support":2,"object":1,"committed":false,
                             "supporters":["checker","lead"]}]})
    );
    let end = Frame::End {
        ending: "exhausted".into(),
        topic: None,
        correct: None,
        rounds: 7,
        turns: 58,
        desk: None,
    };
    assert_eq!(
        serde_json::to_value(&end)?,
        json!({"type":"end","ending":"exhausted","topic":null,"correct":null,"rounds":7,"turns":58})
    );
    Ok(())
}

#[test]
fn compare_frames_pin_their_wire_form() -> Result<()> {
    let seed = CompareFrame::Seed {
        seed: 1,
        problem: "primes-200".into(),
        results: vec![SeedResult {
            package: BASELINE.into(),
            seats: 1,
            ending: "converged".into(),
            correct: false,
            rounds: 2,
            turns: 2,
            topic: Some("47".into()),
        }],
    };
    assert_eq!(
        serde_json::to_value(&seed)?,
        json!({"type":"seed","seed":1,"problem":"primes-200","results":[
            {"package":"single-seat","seats":1,"ending":"converged","correct":false,"rounds":2,"turns":2,"topic":"47"}]})
    );
    let summary = CompareFrame::Summary {
        seeds: 2,
        rows: vec![SummaryRow {
            package: "math-swarm".into(),
            seats: 50,
            accuracy: 0.5,
            converged: 1.0,
            deadlocked: 0.0,
            exhausted: 0.0,
            idle: 0.0,
            mean_rounds: 6.5,
            mean_turns: 51.0,
        }],
    };
    assert_eq!(
        serde_json::to_value(&summary)?,
        json!({"type":"summary","seeds":2,"rows":[{"package":"math-swarm","seats":50,"accuracy":0.5,
               "converged":1.0,"deadlocked":0.0,"exhausted":0.0,"idle":0.0,"mean_rounds":6.5,"mean_turns":51.0}]})
    );
    Ok(())
}
