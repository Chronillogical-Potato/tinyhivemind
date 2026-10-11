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

#[test]
fn routing_frames_pin_their_wire_form() -> Result<()> {
    let accepted = Frame::Referral(ReferralFrame {
        round: 10,
        kind: "report".into(),
        from_desk: "verify".into(),
        to_desk: "synthesis".into(),
        seat: "verify-01".into(),
        target: Some("synthesis-01".into()),
        content: "!propose #924 ^1 Verify settled on 924.\n@#synthesis".into(),
        hops: 4,
        accepted: true,
        reason: None,
        sequence: Some(2),
    });
    assert_eq!(
        serde_json::to_value(&accepted)?,
        json!({"type":"referral","round":10,"kind":"report","from_desk":"verify","to_desk":"synthesis",
               "seat":"verify-01","target":"synthesis-01","content":"!propose #924 ^1 Verify settled on 924.\n@#synthesis",
               "hops":4,"accepted":true,"sequence":2})
    );
    let refused = Frame::Referral(ReferralFrame {
        round: 12,
        kind: "report".into(),
        from_desk: "synthesis".into(),
        to_desk: "conjecture".into(),
        seat: "synthesis-01".into(),
        target: None,
        content: "c".into(),
        hops: 4,
        accepted: false,
        reason: Some("hop_limit_reached".into()),
        sequence: None,
    });
    assert_eq!(
        serde_json::to_value(&refused)?,
        json!({"type":"referral","round":12,"kind":"report","from_desk":"synthesis","to_desk":"conjecture",
               "seat":"synthesis-01","target":null,"content":"c","hops":4,"accepted":false,
               "reason":"hop_limit_reached","sequence":null})
    );
    let desk_end = Frame::DeskEnd {
        desk: "verify".into(),
        ending: "converged".into(),
        topic: Some("924".into()),
        correct: Some(true),
        rounds: 2,
        turns: 11,
    };
    assert_eq!(
        serde_json::to_value(&desk_end)?,
        json!({"type":"desk_end","desk":"verify","ending":"converged","topic":"924","correct":true,"rounds":2,"turns":11})
    );
    let end = Frame::End {
        ending: "converged".into(),
        topic: Some("924".into()),
        correct: Some(true),
        rounds: 12,
        turns: 75,
        desk: Some("synthesis".into()),
    };
    assert_eq!(
        serde_json::to_value(&end)?,
        json!({"type":"end","ending":"converged","topic":"924","correct":true,"rounds":12,"turns":75,"desk":"synthesis"})
    );
    Ok(())
}

#[test]
fn live_frames_pin_their_wire_form() -> Result<()> {
    let start = Frame::Start {
        package: "math-panel".into(),
        seed: 1,
        seats: 5,
        problem: None,
        live: Some(true),
        model: Some("openai/gpt-oss-120b:nitro".into()),
    };
    assert_eq!(
        serde_json::to_value(&start)?,
        json!({"type":"start","package":"math-panel","seed":1,"seats":5,"problem":null,
               "live":true,"model":"openai/gpt-oss-120b:nitro"})
    );
    let usage = Frame::Usage(UsageFrame {
        calls: 12,
        prompt_tokens: 9000,
        completion_tokens: 800,
        total_tokens: 9800,
        cost_usd: 0.0015,
        wall_ms: 4200,
        exhausted_calls: false,
    });
    assert_eq!(
        serde_json::to_value(&usage)?,
        json!({"type":"usage","calls":12,"prompt_tokens":9000,"completion_tokens":800,"total_tokens":9800,
               "cost_usd":0.0015,"wall_ms":4200,"exhausted_calls":false})
    );
    Ok(())
}
