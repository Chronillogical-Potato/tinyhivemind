//! Run the SWE hive or its single-agent baseline on one task.
//!
//! ```sh
//! OPENROUTER_API_KEY=... cargo run --release --bin swe_hive -- \
//!   --mode hive --task "fix the failing test" --container my-sandbox \
//!   --trace out.jsonl --result result.json
//! ```
//!
//! With `--stdio-rpc` commands are exchanged with a parent process as JSON
//! lines on stdout and stdin (see `swe::sandbox`), so stdout carries nothing
//! else and every diagnostic goes to stderr.

use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

use tinyhivemind_core::telemetry::Tracer;
use tinyhivemind_lab::swe::config::{Config, Target};
use tinyhivemind_lab::swe::llm::{CurlChat, Llm};
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
    let tracer = Tracer::new(
        format!("{}-{}", config.mode.name(), std::process::id()),
        &sink,
        &clock,
    );
    let llm = Llm::new(
        Box::new(CurlChat::new(&config.api_base, key, config.request_timeout)),
        &config.model,
        Meter::new(config.token_cap, Some(config.max_turns)),
    );
    let summary = run(&config, &llm, exec.as_ref(), &tracer);
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
