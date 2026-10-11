//! View a hive package, and watch it run offline.
//!
//! ```sh
//! cargo run -p tinyhivemind-lang --example hive_view -- <package-dir>            # text tree
//! cargo run -p tinyhivemind-lang --example hive_view -- <package-dir> --mermaid  # flowchart
//! cargo run -p tinyhivemind-lang --example hive_view -- <package-dir> --json     # structure
//! cargo run -p tinyhivemind-lang --example hive_view -- <package-dir> --run [--frames] [--trace out.jsonl] [--record]
//! cargo run -p tinyhivemind-lang --example hive_view -- <math-package> --compare --seeds 50 [--table]
//! OPENROUTER_API_KEY=... cargo run -p tinyhivemind-lang --example hive_view -- <package-dir> --run --frames --live [--model <id>] [--max-calls N]
//! ```
//!
//! Knobs: `--seed N`, `--round-width N`, `--turn-budget N`, `--problem <id>`.
//! Offline, the episode is model-free: seats speak from a script, a seeded
//! default script, or the math simulation in `sim.json`. With `--live` every
//! seat turn is an `OpenRouter` call. Either way core's `hive::step` decides
//! every round and the ending, and a package of several desks runs one floor
//! per desk, linked only by referral. See `README.md` beside this file.

mod compare;
mod desks;
mod episode;
mod frames;
mod halt;
mod hive;
mod live;
mod load;
mod math;
mod output;
mod render;
mod rng;
mod run;
mod script;
mod sim;
mod structure;

use std::path::PathBuf;

use crate::live::LiveConfig;
use crate::run::Knobs;

/// The example's error type: anything, with its message.
pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Which view the command line asked for. The last mode flag wins, except
/// that `--compare` wins over `--run` (`--run --compare` is a comparison).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Mode {
    #[default]
    Tree,
    Mermaid,
    Json,
    Run,
    Compare,
}

/// What the command line asked for.
#[derive(Debug, Default)]
struct Args {
    dir: Option<PathBuf>,
    mode: Mode,
    output: output::RunOutput,
    trace: Option<PathBuf>,
    table: bool,
    seeds: u32,
    packages: Vec<String>,
    knobs: Knobs,
    live: bool,
    model: Option<String>,
    max_calls: u32,
    max_tokens: u32,
}

impl Args {
    /// The live configuration, when `--live` was given.
    ///
    /// # Errors
    ///
    /// Fails when the model id is refused (see [`live::provider::check_model`]).
    fn live_config(&self, env_model: Option<String>) -> Result<Option<LiveConfig>> {
        if !self.live {
            return Ok(None);
        }
        let model = self
            .model
            .clone()
            .or(env_model.filter(|model| !model.is_empty()))
            .unwrap_or_else(|| live::provider::DEFAULT_MODEL.to_owned());
        live::provider::check_model(&model)?;
        Ok(Some(LiveConfig {
            model,
            max_calls: self.max_calls,
            max_tokens: self.max_tokens,
        }))
    }
}

fn bounded(flag: &str, value: Option<String>, low: u64, high: u64) -> Result<u64> {
    let value = value.ok_or_else(|| format!("{flag} needs a value"))?;
    let parsed: u64 = value
        .parse()
        .map_err(|_| format!("{flag} takes a number, not {value}"))?;
    if parsed < low || parsed > high {
        return Err(format!("{flag} must be in {low}..={high}").into());
    }
    Ok(parsed)
}

fn parse_args(words: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut args = Args {
        seeds: 50,
        max_calls: 300,
        max_tokens: 1200,
        knobs: Knobs {
            seed: 1,
            ..Knobs::default()
        },
        ..Args::default()
    };
    let mut words = words.into_iter();
    while let Some(arg) = words.next() {
        match arg.as_str() {
            "--mermaid" => args.mode = Mode::Mermaid,
            "--json" => args.mode = Mode::Json,
            "--run" if args.mode != Mode::Compare => args.mode = Mode::Run,
            "--run" => {}
            "--frames" => args.output.frames = true,
            "--table" => args.table = true,
            "--compare" => args.mode = Mode::Compare,
            "--trace" => args.trace = Some(words.next().ok_or("--trace needs a path")?.into()),
            "--seeds" => args.seeds = u32::try_from(bounded(&arg, words.next(), 1, 10_000)?)?,
            "--seed" => args.knobs.seed = bounded(&arg, words.next(), 0, u64::MAX)?,
            "--round-width" => {
                args.knobs.round_width = Some(u32::try_from(bounded(&arg, words.next(), 1, 64)?)?);
            }
            "--turn-budget" => {
                args.knobs.turn_budget =
                    Some(u32::try_from(bounded(&arg, words.next(), 1, 2000)?)?);
            }
            "--problem" => args.knobs.problem = Some(words.next().ok_or("--problem needs an id")?),
            "--live" => args.live = true,
            "--record" => args.output.record = true,
            "--model" => {
                let model = words.next().ok_or("--model needs an id")?;
                live::provider::check_model(&model)?;
                args.model = Some(model);
            }
            "--max-calls" => {
                args.max_calls = u32::try_from(bounded(&arg, words.next(), 1, 2000)?)?;
            }
            "--max-tokens" => {
                args.max_tokens = u32::try_from(bounded(&arg, words.next(), 64, 8192)?)?;
            }
            "--packages" => {
                let list = words
                    .next()
                    .ok_or("--packages needs a comma-separated list")?;
                args.packages = list
                    .split(',')
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}").into()),
            _ if args.dir.is_none() => args.dir = Some(arg.into()),
            _ => return Err(format!("unexpected argument {arg}").into()),
        }
    }
    Ok(args)
}

fn main() {
    if let Err(error) = run_main() {
        eprintln!("hive_view: {error}");
        std::process::exit(1);
    }
}

fn run_main() -> Result<()> {
    let args = parse_args(std::env::args().skip(1))?;
    let dir = args.dir.clone().ok_or(
        "usage: hive_view <package-dir> [--mermaid | --json | --run [--frames] | --compare]",
    )?;
    let live = args.live_config(std::env::var("OPENROUTER_MODEL").ok())?;
    if live.is_some() && args.mode != Mode::Run {
        return Err("--live drives a run: use it with --run [--frames]".into());
    }
    if args.output.record && args.mode != Mode::Run {
        return Err("--record keeps a run: use it with --run [--frames]".into());
    }
    if args.mode == Mode::Compare {
        let names = if args.packages.is_empty() {
            compare::DEFAULT_PACKAGES
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        } else {
            args.packages.clone()
        };
        let loaded = compare::packages(&dir, &names)?;
        let stdout = std::io::stdout();
        compare::compare(
            &loaded,
            args.seeds,
            &args.knobs,
            args.table,
            &mut stdout.lock(),
        )?;
        return Ok(());
    }
    let loaded = load::load(&dir)?;
    match args.mode {
        Mode::Run => output::print(
            &loaded,
            &args.knobs,
            args.output,
            args.trace.as_deref(),
            live.as_ref(),
        )?,
        Mode::Json => println!(
            "{}",
            serde_json::to_string(&structure::structure(&loaded)?)?
        ),
        Mode::Mermaid => print!("{}", render::mermaid(&loaded.lowered)),
        Mode::Tree | Mode::Compare => print!("{}", render::tree(&loaded)),
    }
    Ok(())
}

#[cfg(test)]
mod test;
