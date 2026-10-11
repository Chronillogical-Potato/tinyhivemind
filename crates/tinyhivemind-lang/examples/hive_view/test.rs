//! Offline tests for `hive_view`: loading, rendering, runs, the frame wire
//! form, and the comparison. No network, no clock, no temporary files.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Result;
use crate::compare::{BASELINE, compare, packages};
use crate::episode::Ending;
use crate::frames::{
    CompareFrame, Frame, ProblemFrame, SeedResult, StandingFrame, SummaryRow, TurnFrame,
};
use crate::load::{documents, load, summary};
use crate::math::problems;
use crate::render::{mermaid, node};
use crate::run::{Knobs, execute, prepare};

fn gallery(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples/hives")
        .join(name)
}

fn knobs(seed: u64) -> Knobs {
    Knobs {
        seed,
        ..Knobs::default()
    }
}

fn ending(name: &str, seed: u64) -> Result<Ending> {
    let loaded = load(&gallery(name))?;
    let mut prepared = prepare(&loaded, &knobs(seed))?;
    Ok(execute(&mut prepared, None, &mut |_| {})?.ending)
}

#[test]
fn the_loader_rejects_a_directory_without_hive_json() {
    let error = documents(&gallery("review-board").join("roles"))
        .err()
        .map(|error| error.to_string());
    assert!(error.is_some_and(|error| error.contains("has no hive.json")));
}

#[test]
fn the_loader_rejects_a_path_that_is_not_a_directory() {
    let error = documents(&gallery("review-board").join("hive.json"))
        .err()
        .map(|error| error.to_string());
    assert!(error.is_some_and(|error| error.contains("is not a package directory")));
}

#[test]
fn every_gallery_package_loads_and_keeps_its_sidecars_out_of_the_package() -> Result<()> {
    for name in [
        "review-board",
        "research-debate",
        "incident-response",
        "memory-guild",
        "self-improving",
        "math-panel",
        "math-swarm",
    ] {
        let loaded = load(&gallery(name))?;
        assert_eq!(loaded.lowered.id, name);
        assert!(!loaded.summary.is_empty(), "{name} needs a README summary");
        assert!(!loaded.package.documents.contains_key("sim.json"));
        assert!(!loaded.package.documents.contains_key("script.json"));
    }
    Ok(())
}

#[test]
fn a_summary_is_the_first_paragraph_after_the_heading() {
    assert_eq!(
        summary("# title\n\nOne line\nand two.\n\nNot this."),
        "One line and two."
    );
    assert_eq!(summary(""), "");
}

#[test]
fn the_mermaid_view_has_an_edge_for_every_topology_link() -> Result<()> {
    let loaded = load(&gallery("incident-response"))?;
    let chart = mermaid(&loaded.lowered);
    assert!(chart.starts_with("flowchart LR\n"));
    assert!(!loaded.lowered.topology.is_empty());
    for link in &loaded.lowered.topology {
        let edge = format!("  {} --> {}\n", node(&link.from), node(&link.to));
        assert!(chart.contains(&edge), "missing {edge:?} in\n{chart}");
    }
    for link in &loaded.lowered.referrals {
        let edge = format!("  {} -. refer .-> {}\n", node(&link.from), node(&link.to));
        assert!(chart.contains(&edge), "missing {edge:?}");
    }
    for desk in &loaded.lowered.desks {
        assert!(chart.contains(&format!("subgraph {}[", node(&desk.id))));
    }
    Ok(())
}

#[test]
fn review_board_converges_on_merge_under_its_script() -> Result<()> {
    for seed in 1..=3 {
        assert_eq!(
            ending("review-board", seed)?,
            Ending::Converged("merge".into())
        );
    }
    Ok(())
}

#[test]
fn the_seeded_default_script_ends_differently_on_different_seeds() -> Result<()> {
    let endings: Vec<Ending> = (1..=3)
        .map(|seed| ending("incident-response", seed))
        .collect::<Result<_>>()?;
    assert!(
        endings.windows(2).any(|pair| pair[0] != pair[1]),
        "{endings:?}"
    );
    // And the same seed always ends the same way.
    assert_eq!(ending("incident-response", 2)?, endings[1]);
    Ok(())
}

#[test]
fn the_problem_bank_answers_are_computed_exactly() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let bank = problems(loaded.sim.as_ref().ok_or("math-swarm has a sim.json")?)?;
    let expected = [
        ("lattice-6x6", 924),
        ("lattice-5x8", 1287),
        ("modsum-squares", 14),
        ("modsum-cubes", 30),
        ("digitsum-2pow50", 76),
        ("multiples-3-5", 233_168),
        ("sqdiff-20", 41_230),
        ("primes-200", 46),
    ];
    for (id, answer) in expected {
        let problem = bank.iter().find(|problem| problem.id == id).ok_or(id)?;
        assert_eq!(problem.answer()?, answer, "{id}");
        assert_ne!(problem.classic_mistake()?, answer, "{id}");
    }
    Ok(())
}

#[test]
fn a_math_swarm_episode_ends_within_its_turn_budget() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    for seed in 1..=10 {
        let mut prepared = prepare(&loaded, &knobs(seed))?;
        assert_eq!(prepared.room.members.len(), 50);
        let budget = prepared.room.policy.turn_budget;
        let mut widest = 0;
        let outcome = execute(&mut prepared, None, &mut |round| {
            widest = widest.max(round.turns.len());
        })?;
        assert!(
            outcome.turns <= budget,
            "seed {seed}: {} turns over {budget}",
            outcome.turns
        );
        assert!(widest <= 10, "a round wider than round_width");
        assert_ne!(outcome.ending, Ending::Idle);
    }
    Ok(())
}

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
    };
    assert_eq!(
        serde_json::to_value(&start)?,
        json!({"type":"start","package":"math-panel","seed":3,"seats":5,
               "problem":{"id":"lattice-6x6","statement":"s","answer":"924"}})
    );
    let round = Frame::Round {
        round: 1,
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
        json!({"type":"round","round":1,"phase":"deliberate","visibility":"blind",
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
fn compare_streams_one_line_per_seed_then_a_summary_with_the_baseline_first() -> Result<()> {
    let dir = gallery("math-swarm");
    let loaded = packages(&dir, &["math-panel".into(), "math-swarm".into()])?;
    let mut out = Vec::new();
    let rows = compare(&loaded, 3, &Knobs::default(), false, &mut out)?;
    let lines: Vec<Value> = String::from_utf8(out)?
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    assert_eq!(lines.len(), 4);
    for (index, line) in lines[..3].iter().enumerate() {
        assert_eq!(line["type"], "seed");
        assert_eq!(line["seed"], index + 1);
        let packages: Vec<&str> = line["results"]
            .as_array()
            .ok_or("results")?
            .iter()
            .filter_map(|result| result["package"].as_str())
            .collect();
        assert_eq!(packages, [BASELINE, "math-panel", "math-swarm"]);
    }
    assert_eq!(lines[3]["type"], "summary");
    assert_eq!(lines[3]["seeds"], 3);
    let seats: Vec<usize> = rows.iter().map(|row| row.seats).collect();
    assert_eq!(seats, [1, 5, 50]);
    for row in &rows {
        for rate in [
            row.accuracy,
            row.converged,
            row.deadlocked,
            row.exhausted,
            row.idle,
        ] {
            assert!((0.0..=1.0).contains(&rate));
        }
        let endings = row.converged + row.deadlocked + row.exhausted + row.idle;
        assert!((endings - 1.0).abs() < 0.01, "{row:?}");
    }
    Ok(())
}

#[test]
fn compare_refuses_a_package_without_a_simulation() {
    let error = packages(&gallery("math-swarm"), &["review-board".into()])
        .err()
        .map(|error| error.to_string());
    assert!(error.is_some_and(|error| error.contains("has no sim.json")));
}

#[test]
fn knobs_outside_their_bounds_are_rejected() {
    let parse = |argv: &[&str]| crate::parse_args(argv.iter().map(|arg| (*arg).to_owned()));
    assert!(parse(&["dir", "--round-width", "0"]).is_err());
    assert!(parse(&["dir", "--turn-budget", "2001"]).is_err());
    assert!(parse(&["dir", "--seed", "x"]).is_err());
    assert!(parse(&["dir", "--bogus"]).is_err());
    assert!(
        parse(&["dir", "--round-width", "8", "--seed", "4"])
            .is_ok_and(|args| args.knobs.seed == 4 && args.knobs.round_width == Some(8))
    );
}

#[test]
fn the_same_seed_replays_the_same_frames() -> Result<()> {
    let loaded = load(&gallery("math-panel"))?;
    let transcript = |seed| -> Result<Vec<String>> {
        let mut prepared = prepare(&loaded, &knobs(seed))?;
        let mut lines = Vec::new();
        execute(&mut prepared, None, &mut |round| {
            lines.extend(round.turns.iter().map(|turn| turn.content.clone()));
        })?;
        Ok(lines)
    };
    assert_eq!(transcript(7)?, transcript(7)?);
    Ok(())
}
