//! Test-only access to sandbox operations with configurable resource limits.

use std::path::Path;
use std::time::Duration;

use super::{COMMAND_TIMEOUT_SECONDS, DockerSandbox, ShellOutput};

pub(crate) fn shell_with_limits_at(
    sandbox: &DockerSandbox,
    script: &str,
    timeout: Duration,
    max_bytes: u64,
    output: &Path,
) -> anyhow::Result<ShellOutput> {
    sandbox.action_with_limits_at(
        ["timeout", COMMAND_TIMEOUT_SECONDS, "sh", "-lc", script],
        &[],
        timeout,
        max_bytes,
        output,
    )
}
