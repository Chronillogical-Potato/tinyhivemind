//! `run_python`: one fresh, network-less Docker container per program.
//!
//! The confinement follows the DeepSWE sandbox (`deepswe_hive/sandbox.rs`):
//! no network, bounded memory, CPU and process count, every capability
//! dropped, no privilege escalation, an unprivileged user. It goes further in
//! one respect: **nothing is mounted**. The program arrives on stdin, the root
//! filesystem is read-only, and the only writable place is a small tmpfs at
//! `/tmp`, so no host path -- the run directory, the problem file, anything a
//! seat should not read -- is reachable from inside.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
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
            image: std::env::var("EULER_HIVE_PYTHON_IMAGE").unwrap_or_else(|_| DEFAULT_IMAGE.into()),
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
        let stdout = tempfile::NamedTempFile::new()?;
        let stderr = tempfile::NamedTempFile::new()?;
        let started = Instant::now();
        let mut child = Command::new(&self.docker)
            .args(docker_args(&self.image, name))
            .stdin(Stdio::piped())
            .stdout(Stdio::from(stdout.reopen()?))
            .stderr(Stdio::from(stderr.reopen()?))
            .spawn()?;
        if let Some(mut input) = child.stdin.take() {
            input.write_all(program.as_bytes())?;
        }
        // The in-container `timeout` normally ends the program; this host
        // deadline catches a hung Docker CLI as well.
        let deadline = Duration::from_secs(PROGRAM_SECONDS + 15);
        let (code, timed_out) = match child.wait_timeout(deadline)? {
            Some(status) => (status.code(), status.code() == Some(124)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                (None, true)
            }
        };
        // `--rm` removes a container that exited; force it for a killed one.
        let _ = Command::new(&self.docker)
            .args(["rm", "--force", name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Ok(ProgramOutput {
            code,
            stdout: bounded(&std::fs::read(stdout.path())?),
            stderr: bounded(&std::fs::read(stderr.path())?),
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
        "--signal=KILL",
        &seconds,
        "python3",
        "-I",
        "-",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
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
