//! Bounded Docker daemon and sandbox readiness checks.

use std::io::Read;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use wait_timeout::ChildExt;

use super::cleanup::{cleanup_container_id, finish_with_cleanup, terminate};
use super::{
    DockerSandbox, GitLayout, InspectorFailure, SandboxConfig, action_args, command, prepare_mask,
};

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(10);
const PREFLIGHT_OUTPUT_BYTES: u64 = 64 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(5);
static PREFLIGHT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectContainer {
    host_config: InspectHostConfig,
    mounts: Vec<InspectMount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectHostConfig {
    network_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InspectMount {
    destination: String,
    #[serde(rename = "RW")]
    rw: bool,
}

struct CapturedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl DockerSandbox {
    pub(crate) fn preflight(config: SandboxConfig) -> anyhow::Result<Self> {
        let git = GitLayout::discover(&config.repo_path)?;
        preflight(config, git, PREFLIGHT_TIMEOUT, PREFLIGHT_OUTPUT_BYTES)
    }

    pub(crate) fn preflight_from_parent(
        config: SandboxConfig,
        dot_git_is_file: bool,
        git_dir: std::path::PathBuf,
        common_dir: std::path::PathBuf,
    ) -> anyhow::Result<Self> {
        preflight(
            config,
            GitLayout {
                dot_git_is_file,
                git_dir,
                common_dir,
            },
            PREFLIGHT_TIMEOUT,
            PREFLIGHT_OUTPUT_BYTES,
        )
    }
}

fn preflight(
    config: SandboxConfig,
    git: GitLayout,
    timeout: Duration,
    max_bytes: u64,
) -> anyhow::Result<DockerSandbox> {
    let version = capture(
        &config,
        "version",
        ["version", "--format", "{{.Server.Version}}"],
        timeout,
        max_bytes,
    )
    .map_err(|error| anyhow::anyhow!("Docker is unavailable: {error}"))?;
    if !version.status.success() || version.stdout.is_empty() {
        anyhow::bail!("Docker daemon is not active and ready");
    }

    let mask = Arc::new(tempfile::tempdir()?);
    prepare_mask(&mask, git.dot_git_is_file)?;
    let sandbox = DockerSandbox { config, mask, git };
    let control = tempfile::tempdir()?;
    let cidfile = control.path().join("preflight.cid");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let container = format!(
        "deepswe-preflight-{}-{}-{nonce}",
        std::process::id(),
        PREFLIGHT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let result = create_inspect_start(&sandbox, &container, &cidfile, timeout, max_bytes);
    finish_with_cleanup(result, cleanup_container_id(&sandbox.config, &container))?;
    Ok(sandbox)
}

fn create_inspect_start(
    sandbox: &DockerSandbox,
    container: &str,
    cidfile: &std::path::Path,
    timeout: Duration,
    max_bytes: u64,
) -> anyhow::Result<()> {
    let mut create_args = action_args(sandbox, true)?;
    create_args.splice(
        1..1,
        [
            "--name".into(),
            container.into(),
            "--cidfile".into(),
            cidfile.display().to_string(),
        ],
    );
    let git_mask_check = if sandbox.git.dot_git_is_file {
        "test -f /workspace/.git && test ! -s /workspace/.git"
    } else {
        "test -d /workspace/.git && test -z \"$(ls -A /workspace/.git)\""
    };
    let readiness = format!(
        "test -w /workspace && {git_mask_check} && command -v timeout >/dev/null && printf sandbox-ready"
    );
    create_args.extend(["sh".into(), "-c".into(), readiness]);
    let created = capture_vec(&sandbox.config, "create", create_args, timeout, max_bytes)?;
    if !created.status.success() {
        anyhow::bail!(
            "Docker sandbox cannot be created: {}",
            String::from_utf8_lossy(&created.stderr).trim()
        );
    }
    let created_id = String::from_utf8(created.stdout)?.trim().to_owned();
    if created_id.len() < 12 || !created_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("Docker returned an invalid sandbox container id");
    }

    let inspected = capture(
        &sandbox.config,
        "inspect",
        ["inspect", container],
        timeout,
        max_bytes,
    )?;
    if !inspected.status.success() {
        anyhow::bail!(
            "Docker inspect failed: {}",
            String::from_utf8_lossy(&inspected.stderr).trim()
        );
    }
    validate_inspection(&inspected.stdout)?;
    let started = capture(
        &sandbox.config,
        "start",
        ["start", "--attach", container],
        timeout,
        max_bytes,
    )?;
    if !started.status.success() || started.stdout != b"sandbox-ready" {
        anyhow::bail!(
            "Docker sandbox did not report active/ready: {}",
            String::from_utf8_lossy(&started.stderr).trim()
        );
    }
    Ok(())
}

fn validate_inspection(bytes: &[u8]) -> anyhow::Result<()> {
    let rows: Vec<InspectContainer> = serde_json::from_slice(bytes)?;
    let row = rows
        .first()
        .ok_or_else(|| anyhow::anyhow!("Docker inspect returned no container"))?;
    let workspace = row
        .mounts
        .iter()
        .find(|mount| mount.destination == "/workspace");
    let git = row
        .mounts
        .iter()
        .find(|mount| mount.destination == "/workspace/.git");
    if row.host_config.network_mode != "none"
        || !matches!(workspace, Some(mount) if mount.rw)
        || !matches!(git, Some(mount) if !mount.rw)
        || row.mounts.len() != 2
    {
        anyhow::bail!(
            "Docker did not report exactly a no-network writable workspace and read-only Git mask"
        );
    }
    Ok(())
}

fn capture<const N: usize>(
    config: &SandboxConfig,
    phase: &'static str,
    args: [&str; N],
    timeout: Duration,
    max_bytes: u64,
) -> anyhow::Result<CapturedOutput> {
    capture_vec(
        config,
        phase,
        args.into_iter().map(str::to_owned).collect(),
        timeout,
        max_bytes,
    )
}

fn capture_vec(
    config: &SandboxConfig,
    phase: &'static str,
    args: Vec<String>,
    timeout: Duration,
    max_bytes: u64,
) -> anyhow::Result<CapturedOutput> {
    let mut child = command(config)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = bounded_reader(
        child.stdout.take().expect("piped stdout"),
        max_bytes,
        Arc::clone(&overflow),
    );
    let stderr = bounded_reader(
        child.stderr.take().expect("piped stderr"),
        max_bytes,
        Arc::clone(&overflow),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            terminate(&mut child);
            join_reader(stdout)?;
            join_reader(stderr)?;
            return Err(InspectorFailure::PreflightOutputTooLarge { phase, max_bytes }.into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            terminate(&mut child);
            join_reader(stdout)?;
            join_reader(stderr)?;
            return Err(InspectorFailure::PreflightTimedOut { phase }.into());
        }
        if let Some(status) = child.wait_timeout(remaining.min(POLL_INTERVAL))? {
            break status;
        }
    };
    let stdout = join_reader(stdout)?;
    let stderr = join_reader(stderr)?;
    if overflow.load(Ordering::Acquire) {
        return Err(InspectorFailure::PreflightOutputTooLarge { phase, max_bytes }.into());
    }
    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
    })
}

fn bounded_reader(
    mut pipe: impl Read + Send + 'static,
    max_bytes: u64,
    overflow: Arc<AtomicBool>,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut captured = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let read = pipe.read(&mut chunk)?;
            if read == 0 {
                return Ok(captured);
            }
            let remaining = max_bytes
                .saturating_add(1)
                .saturating_sub(captured.len() as u64);
            captured.extend_from_slice(&chunk[..read.min(remaining as usize)]);
            if captured.len() as u64 > max_bytes {
                overflow.store(true, Ordering::Release);
                return Ok(captured);
            }
        }
    })
}

fn join_reader(
    reader: std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> anyhow::Result<Vec<u8>> {
    reader
        .join()
        .map_err(|_| anyhow::anyhow!("Docker output reader panicked"))?
        .map_err(Into::into)
}

#[cfg(test)]
pub(crate) fn preflight_with_limits(
    config: SandboxConfig,
    timeout: Duration,
    max_bytes: u64,
) -> anyhow::Result<DockerSandbox> {
    let git = GitLayout::discover(&config.repo_path)?;
    preflight(config, git, timeout, max_bytes)
}
