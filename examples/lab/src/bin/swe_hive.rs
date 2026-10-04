//! Run the SWE hive or its single-agent baseline on one task.
//!
//! ```sh
//! OPENROUTER_API_KEY=... cargo run --release --bin swe_hive -- \
//!   --mode hive --task "fix the failing test" --container my-sandbox \
//!   --trace out.jsonl --result result.json
//! ```
//!
//! With `--memory cortex` seats store and recall what they did through a
//! CortexDB server (`--memory-url` or `CORTEX_DB_URL`, key from
//! `CORTEX_DB_KEY`), under the run's own namespace root `team:<run-id>`.
//!
//! With `--stdio-rpc` commands are exchanged with a parent process as JSON
//! lines on stdout and stdin (see `swe::sandbox`), so stdout carries nothing
//! else and every diagnostic goes to stderr.

use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

use tinyhivemind_core::telemetry::{TraceEvent, Tracer};
use tinyhivemind_lab::swe::config::{Config, MemoryKind, Mode, Target};
use tinyhivemind_lab::swe::llm::{CurlChat, Llm};
use tinyhivemind_lab::swe::memory::{HiveMemory, SeatMemory, generated_run_id};
use tinyhivemind_lab::swe::roles::Role;
use tinyhivemind_lab::swe::single;
use tinyhivemind_lab::swe::meter::Meter;
use tinyhivemind_lab::swe::run::run;
use tinyhivemind_lab::swe::sandbox::{DockerExec, Exec, StdioExec};
use tinyhivemind_lab::{JsonlSink, WallClock};

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("swe_hive: {message}");
            ExitCode::from(2)
        }
    }
}

fn real_main() -> Result<(), String> {
    let config = Config::parse(std::env::args().skip(1)).map_err(|error| error.to_string())?;
    // The key comes from the environment only and is never printed or logged.
    let key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if key.is_empty() && config.api_base.starts_with("https://") {
        return Err("OPENROUTER_API_KEY is not set".into());
    }
    let exec: Box<dyn Exec> = match &config.target {
        Target::Container(name) => Box::new(DockerExec::new(name)),
        Target::StdioRpc => Box::new(StdioExec::new(
            io::BufReader::new(io::stdin()),
            io::stdout(),
        )),
    };
    let writer: Box<dyn Write + Send> = match &config.trace {
        Some(path) => Box::new(
            File::create(path).map_err(|error| format!("cannot create trace file: {error}"))?,
        ),
        None => Box::new(io::sink()),
    };
    let sink = JsonlSink::new(writer);
    let clock = WallClock::default();
    let run_id = config
        .run_id
        .clone()
        .unwrap_or_else(|| generated_run_id(config.mode.name()));
    let memory = open_memory(&config, &run_id)?;
    let tracer = Tracer::new(run_id, &sink, &clock);
    if let Some(memory) = &memory {
        tracer.emit(TraceEvent::Mark {
            label: "memory".into(),
            detail: format!("open {}", memory.describe()),
        });
    }
    let llm = Llm::new(
        Box::new(CurlChat::new(&config.api_base, key, config.request_timeout)),
        &config.model,
        Meter::new(config.token_cap, Some(config.max_turns)),
    );
    let summary = run(
        &config,
        &llm,
        exec.as_ref(),
        &tracer,
        memory.as_ref().map(|memory| memory as &dyn SeatMemory),
    );
    let document = summary.to_json();
    if let Some(path) = &config.result {
        std::fs::write(path, format!("{document:#}\n"))
            .map_err(|error| format!("cannot write result: {error}"))?;
    }
    eprintln!("{document}");
    if matches!(config.target, Target::StdioRpc) {
        // One final line the parent can recognise; it has no `exec` key.
        println!(
            "{}",
            serde_json::json!({ "done": true, "result": document })
        );
        let _ = io::stdout().flush();
    }
    Ok(())
}

/// The run's memory, when `--memory cortex` asks for one. The key comes from
/// `CORTEX_DB_KEY` only and is never printed.
fn open_memory(config: &Config, run_id: &str) -> Result<Option<HiveMemory>, String> {
    if config.memory == MemoryKind::None {
        return Ok(None);
    }
    let url = config
        .memory_url
        .clone()
        .or_else(|| std::env::var("CORTEX_DB_URL").ok())
        .filter(|url| !url.trim().is_empty())
        .ok_or("--memory cortex needs --memory-url or CORTEX_DB_URL")?;
    let key = std::env::var("CORTEX_DB_KEY").unwrap_or_default();
    if key.is_empty() {
        return Err("CORTEX_DB_KEY is not set".into());
    }
    let seats: Vec<&str> = match config.mode {
        Mode::Hive => Role::ALL.iter().map(|role| role.id()).collect(),
        Mode::Single => vec![single::SEAT],
    };
    HiveMemory::cortex(&url, &key, run_id, &seats, config.memory_budget).map(Some)
}
