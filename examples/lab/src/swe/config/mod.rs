//! Command-line configuration for `swe_hive`.
//!
//! Hand-rolled because the lab takes no argument-parsing dependency. The API
//! key is deliberately not a flag: it is read from `OPENROUTER_API_KEY` by the
//! binary, so it never appears in a process argument list or a shell history.

use std::fmt;
use std::path::PathBuf;

/// Which arm to run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// Four seats on a shared desk.
    Hive,
    /// One seat, one conversation.
    Single,
}

impl Mode {
    /// The name used in `result.json` and the CLI.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hive => "hive",
            Self::Single => "single",
        }
    }
}

/// Where commands run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// `docker exec` into this container.
    Container(String),
    /// JSON-lines RPC over stdout/stdin.
    StdioRpc,
}

/// A parsed command line.
#[derive(Clone, Debug)]
pub struct Config {
    /// Arm.
    pub mode: Mode,
    /// The task instruction.
    pub task: String,
    /// Where commands run.
    pub target: Target,
    /// Model name.
    pub model: String,
    /// API base URL, without `/chat/completions`.
    pub api_base: String,
    /// JSONL trace output.
    pub trace: Option<PathBuf>,
    /// `result.json` output.
    pub result: Option<PathBuf>,
    /// Most model calls, all seats together.
    pub max_turns: u64,
    /// Most seats running at once.
    pub round_width: usize,
    /// Token cap, input plus output.
    pub token_cap: Option<u64>,
    /// Model calls per hive activation.
    pub steps_per_turn: usize,
    /// Seconds per sandbox command.
    pub cmd_timeout: u64,
    /// Bytes of command output shown to the model.
    pub output_limit: usize,
    /// Seconds per model request.
    pub request_timeout: u64,
}

/// A bad command line.
#[derive(Debug, Eq, PartialEq)]
pub struct UsageError(pub String);

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n\n{USAGE}", self.0)
    }
}

impl std::error::Error for UsageError {}

/// The help text.
pub const USAGE: &str = "usage: swe_hive --mode hive|single (--task TEXT | --task-file F) \
(--container NAME | --stdio-rpc)\n  [--model M] [--api-base URL] [--trace F] [--result F]\n  \
[--max-turns N] [--round-width N] [--token-cap N] [--steps-per-turn N]\n  \
[--cmd-timeout SECS] [--output-limit BYTES] [--request-timeout SECS]\n\
The API key is read from OPENROUTER_API_KEY.";

impl Config {
    /// Parse arguments (without the program name).
    ///
    /// # Errors
    ///
    /// Returns a [`UsageError`] naming the problem.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, UsageError> {
        let mut mode = None;
        let mut task = None;
        let mut container = None;
        let mut rpc = false;
        let mut config = Self {
            mode: Mode::Hive,
            task: String::new(),
            target: Target::StdioRpc,
            model: "openai/gpt-oss-120b:nitro".into(),
            api_base: "https://openrouter.ai/api/v1".into(),
            trace: None,
            result: None,
            max_turns: 60,
            round_width: 2,
            token_cap: None,
            steps_per_turn: 12,
            cmd_timeout: 180,
            output_limit: 6000,
            request_timeout: 240,
        };
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            if flag == "--stdio-rpc" {
                rpc = true;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| UsageError(format!("{flag} needs a value")))?;
            match flag.as_str() {
                "--mode" => mode = Some(parse_mode(&value)?),
                "--task" => task = Some(value),
                "--task-file" => {
                    task = Some(std::fs::read_to_string(&value).map_err(|error| {
                        UsageError(format!("cannot read task file {value}: {error}"))
                    })?);
                }
                "--container" => container = Some(value),
                "--model" => config.model = value,
                "--api-base" => config.api_base = value,
                "--trace" => config.trace = Some(value.into()),
                "--result" => config.result = Some(value.into()),
                "--max-turns" => config.max_turns = number(&flag, &value)?,
                "--round-width" => config.round_width = number(&flag, &value)?,
                "--token-cap" => config.token_cap = Some(number(&flag, &value)?),
                "--steps-per-turn" => config.steps_per_turn = number(&flag, &value)?,
                "--cmd-timeout" => config.cmd_timeout = number(&flag, &value)?,
                "--output-limit" => config.output_limit = number(&flag, &value)?,
                "--request-timeout" => config.request_timeout = number(&flag, &value)?,
                other => return Err(UsageError(format!("unknown flag {other}"))),
            }
        }
        config.mode = mode.ok_or_else(|| UsageError("--mode is required".into()))?;
        config.task = task
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| UsageError("--task or --task-file is required".into()))?;
        config.target = match (container, rpc) {
            (Some(name), false) => Target::Container(name),
            (None, true) => Target::StdioRpc,
            _ => {
                return Err(UsageError(
                    "give exactly one of --container or --stdio-rpc".into(),
                ));
            }
        };
        if config.round_width == 0 || config.max_turns == 0 || config.steps_per_turn == 0 {
            return Err(UsageError(
                "--round-width, --max-turns and --steps-per-turn must be at least 1".into(),
            ));
        }
        Ok(config)
    }
}

fn parse_mode(value: &str) -> Result<Mode, UsageError> {
    match value {
        "hive" => Ok(Mode::Hive),
        "single" => Ok(Mode::Single),
        other => Err(UsageError(format!(
            "--mode must be hive or single, not {other}"
        ))),
    }
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, UsageError> {
    value
        .parse()
        .map_err(|_| UsageError(format!("{flag} needs a number, not {value}")))
}

#[cfg(test)]
mod test;
