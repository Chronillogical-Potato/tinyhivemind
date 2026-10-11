//! `--compare`: the single-seat baseline and each math package over the same
//! seeds and problems, scored against the exact answers.

use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::Result;
use crate::episode::{Ending, Outcome};
use crate::frames::{CompareFrame, SeedResult, SummaryRow};
use crate::load::{Loaded, load};
use crate::math::problems;
use crate::run::{Knobs, Prepared, baseline, correct, execute, pick, prepare};

/// The packages `--compare` runs when `--packages` is not given.
pub(crate) const DEFAULT_PACKAGES: [&str; 2] = ["math-panel", "math-swarm"];

/// The baseline's package name in every result.
pub(crate) const BASELINE: &str = "single-seat";

#[derive(Debug, Default)]
struct Tally {
    package: String,
    seats: usize,
    correct: u32,
    endings: [u32; 4],
    rounds: u64,
    turns: u64,
    elapsed: Duration,
}

impl Tally {
    fn add(&mut self, result: &SeedResult, elapsed: Duration) {
        self.correct += u32::from(result.correct);
        let slot = match result.ending.as_str() {
            "converged" => 0,
            "deadlocked" => 1,
            "exhausted" => 2,
            _ => 3,
        };
        self.endings[slot] += 1;
        self.rounds += u64::from(result.rounds);
        self.turns += u64::from(result.turns);
        self.elapsed += elapsed;
    }

    fn row(&self, seeds: u32) -> SummaryRow {
        let rate = |count: u32| ratio(f64::from(count), seeds);
        let mean = |total: u64| ratio(f64::from(u32::try_from(total).unwrap_or(u32::MAX)), seeds);
        SummaryRow {
            package: self.package.clone(),
            seats: self.seats,
            accuracy: rate(self.correct),
            converged: rate(self.endings[0]),
            deadlocked: rate(self.endings[1]),
            exhausted: rate(self.endings[2]),
            idle: rate(self.endings[3]),
            mean_rounds: mean(self.rounds),
            mean_turns: mean(self.turns),
        }
    }
}

/// `value / seeds`, rounded to three decimals.
fn ratio(value: f64, seeds: u32) -> f64 {
    (value / f64::from(seeds.max(1)) * 1000.0).round() / 1000.0
}

fn result(package: &str, mut prepared: Prepared) -> Result<(SeedResult, Duration)> {
    let started = Instant::now();
    let outcome: Outcome = execute(&mut prepared, None, &mut |_| {})?;
    let elapsed = started.elapsed();
    let correct = correct(&outcome, prepared.problem.as_ref()).unwrap_or(false);
    Ok((
        SeedResult {
            package: package.to_owned(),
            seats: prepared.room.members.len(),
            ending: outcome.ending.name().to_owned(),
            correct,
            rounds: outcome.rounds,
            turns: outcome.turns,
            topic: match &outcome.ending {
                Ending::Converged(topic) => Some(topic.clone()),
                _ => None,
            },
        },
        elapsed,
    ))
}

/// Resolve `--packages` names as siblings of `dir` (or `dir` itself).
///
/// # Errors
///
/// Fails when a package does not load or carries no `sim.json`.
pub(crate) fn packages(dir: &Path, names: &[String]) -> Result<Vec<Loaded>> {
    let parent = dir.parent().unwrap_or(dir);
    names
        .iter()
        .map(|name| {
            let path = if dir.file_name().is_some_and(|own| own == name.as_str()) {
                dir.to_path_buf()
            } else {
                parent.join(name)
            };
            let loaded = load(&path)?;
            if loaded.sim.is_none() {
                return Err(format!("{name} has no sim.json; --compare runs math packages").into());
            }
            Ok(loaded)
        })
        .collect()
}

/// Run the comparison and write NDJSON (or a table) to `out`.
///
/// The problem bank and the baseline come from the first package; every
/// package must hold the problem each seed picks.
///
/// # Errors
///
/// Fails on any load or run error.
pub(crate) fn compare(
    loaded: &[Loaded],
    seeds: u32,
    knobs: &Knobs,
    table: bool,
    out: &mut dyn Write,
) -> Result<Vec<SummaryRow>> {
    let first = loaded
        .first()
        .ok_or("--compare needs at least one package")?;
    let bank = problems(
        first
            .sim
            .as_ref()
            .ok_or("the first package has no sim.json")?,
    )?;
    let mut tallies: Vec<Tally> = std::iter::once(BASELINE.to_owned())
        .chain(loaded.iter().map(|package| package.lowered.id.clone()))
        .map(|package| Tally {
            package,
            ..Tally::default()
        })
        .collect();
    for seed in 1..=u64::from(seeds) {
        let problem = pick(&bank, seed, knobs.problem.as_deref())?;
        let knobs = Knobs {
            seed,
            problem: Some(problem.id.clone()),
            ..knobs.clone()
        };
        let mut results = vec![result(BASELINE, baseline(first, &knobs)?)?];
        for package in loaded {
            results.push(result(&package.lowered.id, prepare(package, &knobs)?)?);
        }
        for (tally, (result, elapsed)) in tallies.iter_mut().zip(&results) {
            tally.seats = result.seats;
            tally.add(result, *elapsed);
        }
        if !table {
            let frame = CompareFrame::Seed {
                seed,
                problem: problem.id.clone(),
                results: results.into_iter().map(|(result, _)| result).collect(),
            };
            writeln!(out, "{}", serde_json::to_string(&frame)?)?;
        }
    }
    let rows: Vec<SummaryRow> = tallies.iter().map(|tally| tally.row(seeds)).collect();
    if table {
        print_table(out, &tallies, &rows, seeds)?;
    } else {
        let summary = CompareFrame::Summary {
            seeds: u64::from(seeds),
            rows: rows.clone(),
        };
        writeln!(out, "{}", serde_json::to_string(&summary)?)?;
    }
    Ok(rows)
}

fn print_table(
    out: &mut dyn Write,
    tallies: &[Tally],
    rows: &[SummaryRow],
    seeds: u32,
) -> Result<()> {
    writeln!(
        out,
        "{seeds} seeds, same problem per seed for every package; correct = exact answer\n"
    )?;
    writeln!(
        out,
        "{:<12} {:>5} {:>8} {:>9} {:>10} {:>9} {:>6} {:>7} {:>7} {:>11}",
        "package",
        "seats",
        "accuracy",
        "converged",
        "deadlocked",
        "exhausted",
        "idle",
        "rounds",
        "turns",
        "ms/episode"
    )?;
    for (tally, row) in tallies.iter().zip(rows) {
        let millis = tally.elapsed.as_secs_f64() * 1000.0 / f64::from(seeds.max(1));
        writeln!(
            out,
            "{:<12} {:>5} {:>8.3} {:>9.3} {:>10.3} {:>9.3} {:>6.3} {:>7.2} {:>7.2} {:>11.3}",
            row.package,
            row.seats,
            row.accuracy,
            row.converged,
            row.deadlocked,
            row.exhausted,
            row.idle,
            row.mean_rounds,
            row.mean_turns,
            millis
        )?;
    }
    Ok(())
}
