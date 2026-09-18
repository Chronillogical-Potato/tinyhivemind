//! Bounded Docker container removal and absence verification.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use wait_timeout::ChildExt;

use super::{InspectorFailure, SandboxConfig, command};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn cleanup_container(
    config: &SandboxConfig,
    cidfile: &Path,
    container_name: &str,
) -> anyhow::Result<()> {
    let id = std::fs::read_to_string(cidfile)
        .ok()
        .and_then(|id| {
            let id = id.trim();
            (!id.is_empty()).then(|| id.to_owned())
        })
        .unwrap_or_else(|| container_name.to_owned());
    cleanup_container_id(config, &id)
}

pub(super) fn cleanup_container_id(config: &SandboxConfig, id: &str) -> anyhow::Result<()> {
    let removal = bounded_docker(config, ["rm", "--force", id], CLEANUP_TIMEOUT);
    let verification = bounded_docker(config, ["inspect", id], CLEANUP_TIMEOUT);
    match (removal, verification) {
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(removed), Ok(inspected)) if inspected.success => {
            let detail = if removed.success {
                format!("container {id} still exists after force removal")
            } else {
                String::from_utf8_lossy(&removed.stderr).trim().to_owned()
            };
            Err(InspectorFailure::CleanupFailed { detail }.into())
        }
        (Ok(_), Ok(inspected)) if proves_absence(&inspected.stderr, id) => Ok(()),
        (Ok(_), Ok(inspected)) => {
            let detail = String::from_utf8_lossy(&inspected.stderr).trim().to_owned();
            Err(InspectorFailure::CleanupFailed {
                detail: if detail.is_empty() {
                    format!("cannot prove container {id} is absent")
                } else {
                    format!("cannot prove container {id} is absent: {detail}")
                },
            }
            .into())
        }
    }
}

fn proves_absence(stderr: &[u8], id: &str) -> bool {
    let Ok(stderr) = std::str::from_utf8(stderr) else {
        return false;
    };
    let diagnostic = stderr.trim();
    [
        format!("Error: No such object: {id}"),
        format!("Error: No such container: {id}"),
        format!("Error response from daemon: No such object: {id}"),
        format!("Error response from daemon: No such container: {id}"),
    ]
    .iter()
    .any(|expected| diagnostic.eq_ignore_ascii_case(expected))
}

struct BoundedOutput {
    success: bool,
    stderr: Vec<u8>,
}

fn bounded_docker<const N: usize>(
    config: &SandboxConfig,
    args: [&str; N],
    timeout: Duration,
) -> anyhow::Result<BoundedOutput> {
    let stdout = tempfile::NamedTempFile::new()?;
    let stderr = tempfile::NamedTempFile::new()?;
    let mut child = command(config)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout.reopen()?))
        .stderr(Stdio::from(stderr.reopen()?))
        .spawn()
        .map_err(|error| InspectorFailure::CleanupFailed {
            detail: error.to_string(),
        })?;
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            terminate(&mut child);
            return Err(InspectorFailure::CleanupTimeout {
                seconds: timeout.as_secs(),
            }
            .into());
        }
        Err(error) => {
            terminate(&mut child);
            return Err(InspectorFailure::CleanupFailed {
                detail: error.to_string(),
            }
            .into());
        }
    };
    Ok(BoundedOutput {
        success: status.success(),
        stderr: read_bounded(stderr.path(), 64 * 1024)?,
    })
}

pub(super) fn terminate(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub(super) fn finish_with_cleanup<T>(
    result: anyhow::Result<T>,
    cleanup: anyhow::Result<()>,
) -> anyhow::Result<T> {
    cleanup?;
    result
}

pub(super) fn read_bounded(path: &Path, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}
