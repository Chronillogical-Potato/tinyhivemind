//! `--compare`: its stream, its summary, and what it refuses.

use serde_json::Value;

use crate::Result;
use crate::compare::{BASELINE, compare, packages};
use crate::run::Knobs;

use super::support::gallery;

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
fn compare_refuses_arms_that_define_the_same_problem_id_differently() -> Result<()> {
    let dir = gallery("math-swarm");
    let first = packages(&dir, &["math-panel".into()])?;
    let mut second = packages(&dir, &["math-swarm".into()])?;
    let sim = second[0].sim.as_mut().ok_or("sim")?;
    let bank = sim["problems"].as_array_mut().ok_or("bank")?;
    for problem in bank {
        problem["statement"] = Value::from("a different question under the same id");
    }
    let mut loaded = first;
    loaded.append(&mut second);
    let knobs = Knobs::default();
    let error = compare(&loaded, 2, &knobs, false, &mut Vec::new())
        .err()
        .map(|error| error.to_string());
    assert!(
        error.is_some_and(|error| error.contains("differently")),
        "arms must share the same problem"
    );
    Ok(())
}
