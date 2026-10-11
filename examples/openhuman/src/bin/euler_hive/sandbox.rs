//! `run_python`: one fresh, network-less Docker container per program.
//!
//! The confinement follows the DeepSWE sandbox (`deepswe_hive/sandbox.rs`):
//! no network, bounded memory, CPU and process count, every capability
//! dropped, no privilege escalation, an unprivileged user. It goes further in
//! one respect: **nothing is mounted**. The program arrives on stdin, the root
//! filesystem is read-only, and the only writable place is a small tmpfs at
//! `/tmp`, so no host path -- the run directory, the problem file, anything a
//! seat should not read -- is reachable from inside.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

/// The image the sandbox runs; `EULER_HIVE_PYTHON_IMAGE` overrides it.
pub(super) const DEFAULT_IMAGE: &str = "python:3.12-slim";
/// What one program may take, wall clock, before the container is killed.
pub(super) const PROGRAM_SECONDS: u64 = 120;
/// Output kept from each stream; the rest is truncated.
pub(super) const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// The largest program accepted.
pub(super) const MAX_PROGRAM_BYTES: usize = 64 * 1024;

/// The Docker command and image the sandbox drives.
#[derive(Clone, Debug)]
pub(super) struct PythonSandbox {
    docker: PathBuf,
    image: String,
}

/// What one program did.
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct ProgramOutput {
    /// The process exit code; `None` when it was killed.
    pub(super) code: Option<i32>,
    /// Standard output, truncated to [`MAX_OUTPUT_BYTES`].
    pub(super) stdout: String,
    /// Standard error, truncated to [`MAX_OUTPUT_BYTES`].
    pub(super) stderr: String,
    /// Whether the wall-clock limit killed it.
    pub(super) timed_out: bool,
    /// How long it ran, in milliseconds.
    pub(super) elapsed_ms: u128,
}

impl PythonSandbox {
    /// The sandbox from the environment: `docker` on `PATH` and the default
    /// image unless `EULER_HIVE_PYTHON_IMAGE` names another.
    pub(super) fn from_env() -> Self {
        Self {
            docker: PathBuf::from("docker"),
            image: std::env::var("EULER_HIVE_PYTHON_IMAGE")
                .unwrap_or_else(|_| DEFAULT_IMAGE.into()),
        }
    }

    /// The image this sandbox runs.
    pub(super) fn image(&self) -> &str {
        &self.image
    }

    /// Check Docker answers and the image is present, before any seat runs.
    ///
    /// # Errors
    ///
    /// Fails when the daemon is unreachable or the image is missing.
    pub(super) fn preflight(&self) -> anyhow::Result<()> {
        let output = Command::new(&self.docker)
            .args(["image", "inspect", "--format", "{{.Id}}", &self.image])
            .stdin(Stdio::null())
            .output()?;
        if !output.status.success() {
            anyhow::bail!(
                "docker image {} is unavailable: {}",
                self.image,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Run `program` to completion or to the wall-clock limit.
    ///
    /// # Errors
    ///
    /// Fails on an oversized program or when Docker cannot be started.
    pub(super) fn run(&self, program: &str, name: &str) -> anyhow::Result<ProgramOutput> {
        if program.len() > MAX_PROGRAM_BYTES {
            anyhow::bail!("program exceeds {MAX_PROGRAM_BYTES} bytes");
        }
        let started = Instant::now();
        let mut child = Command::new(&self.docker)
            .args(docker_args(&self.image, name))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(program.as_bytes())?;
        }
        // Output is read into bounded buffers while the program runs; one
        // byte past the bound on either stream stops the container, so a
        // print loop cannot fill the host's disk or memory.
        let overflow = Arc::new(AtomicBool::new(false));
        let readers = [
            child
                .stdout
                .take()
                .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        ]
        .map(|pipe| {
            let overflow = overflow.clone();
            std::thread::spawn(move || {
                pipe.map_or_else(Vec::new, |pipe| {
                    read_bounded(pipe, MAX_OUTPUT_BYTES, &overflow)
                })
            })
        });
        // The in-container `timeout` normally ends the program; this host
        // deadline catches a hung Docker CLI as well.
        let deadline = Duration::from_secs(PROGRAM_SECONDS + 15);
        let (code, timed_out) = loop {
            if let Some(status) = child.wait_timeout(Duration::from_millis(200))? {
                break (status.code(), status.code() == Some(124));
            }
            if overflow.load(Ordering::Acquire) || started.elapsed() >= deadline {
                let _ = Command::new(&self.docker)
                    .args(["kill", name])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                break (None, !overflow.load(Ordering::Acquire));
            }
        };
        let [stdout, stderr] = readers.map(|reader| reader.join().unwrap_or_default());
        let flooded = overflow.load(Ordering::Acquire);
        // `--rm` removes a container that exited; force it for a killed one.
        let _ = Command::new(&self.docker)
            .args(["rm", "--force", name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Ok(ProgramOutput {
            code,
            stdout: bounded(&stdout),
            stderr: if flooded {
                format!(
                    "{}\n[output passed {MAX_OUTPUT_BYTES} bytes; the program was stopped]",
                    bounded(&stderr)
                )
            } else {
                bounded(&stderr)
            },
            timed_out,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

/// The exact `docker` arguments for one program. Pure, so the confinement is
/// pinned by test rather than by inspection.
pub(super) fn docker_args(image: &str, name: &str) -> Vec<String> {
    let seconds = PROGRAM_SECONDS.to_string();
    [
        "run",
        "--rm",
        "--interactive",
        "--name",
        name,
        "--network",
        "none",
        "--memory",
        "1g",
        "--memory-swap",
        "1g",
        "--cpus",
        "2",
        "--pids-limit",
        "128",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,nosuid,nodev,size=64m",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--log-driver",
        "none",
        "--user",
        "65534:65534",
        "--workdir",
        "/tmp",
        "--env",
        "HOME=/tmp",
        "--env",
        "PYTHONDONTWRITEBYTECODE=1",
        image,
        "timeout",
        &seconds,
        "python3",
        "-I",
        "-",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Read `pipe` to its end, keeping at most `limit` bytes. Reading past the
/// limit sets `overflow` (so the caller stops the program) and keeps draining
/// without storing, so the writer never blocks on a full pipe.
pub(super) fn read_bounded(mut pipe: impl Read, limit: usize, overflow: &AtomicBool) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let room = limit.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..read.min(room)]);
                if read > room {
                    overflow.store(true, Ordering::Release);
                }
            }
        }
    }
    kept
}

fn bounded(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= MAX_OUTPUT_BYTES {
        return text.into_owned();
    }
    let mut end = MAX_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated {} bytes]", &text[..end], text.len() - end)
}
