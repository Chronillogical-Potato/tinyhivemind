//! Where a seat's `bash` tool runs.
//!
//! [`Exec`] is the one seam: given a command line and a timeout, return its
//! merged output and exit code. [`DockerExec`] runs it in a named container
//! with `docker exec`; [`StdioExec`] sends it as a JSON line to whoever is on
//! the other end of our stdout (the Harbor agent, which runs it inside the
//! task environment). [`refuse`] is the policy that blocks a command before it
//! is sent anywhere, and [`truncate`] bounds what comes back.

mod docker;
mod rpc;

use std::time::Duration;

pub use docker::DockerExec;
pub use rpc::{Reply, StdioExec, encode_request};

/// The result of one command.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExecOutput {
    /// Merged stdout and stderr.
    pub stdout: String,
    /// The exit code; 124 for a timeout.
    pub exit: i32,
}

/// Something that can run a shell command.
pub trait Exec: Send + Sync {
    /// Run `cmd` under bash, giving up after `timeout`.
    ///
    /// # Errors
    ///
    /// Returns a message when the command could not be run at all; a command
    /// that ran and failed is an `Ok` with a non-zero exit.
    fn exec(&self, cmd: &str, timeout: Duration) -> Result<ExecOutput, String>;
}

/// The longest command a seat may send.
pub const MAX_COMMAND_CHARS: usize = 20_000;

/// Why a command must not run, or `None` when it may.
///
/// The sandbox is the real boundary; this only stops the plainly
/// self-destructive and the empty, with a reason the seat can read.
#[must_use]
pub fn refuse(cmd: &str) -> Option<&'static str> {
    let flat: String = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return Some("empty command");
    }
    if cmd.len() > MAX_COMMAND_CHARS {
        return Some("command too long; write it to a file in smaller pieces");
    }
    const BLOCKED: &[(&str, &str)] = &[
        ("rm -rf /*", "refuses to wipe the filesystem root"),
        ("rm -rf / ", "refuses to wipe the filesystem root"),
        (":(){", "fork bombs are refused"),
        ("mkfs", "formatting devices is refused"),
        ("shutdown", "power control is refused"),
        ("reboot", "power control is refused"),
        ("> /dev/sd", "writing raw devices is refused"),
    ];
    let padded = format!("{flat} ");
    BLOCKED
        .iter()
        .find(|(needle, _)| padded.contains(needle))
        .map(|(_, why)| *why)
}

/// Keep the head and tail of `text` within `max` bytes, on char boundaries.
///
/// Test failures and stack traces put the useful part at the end, so the tail
/// is kept as well as the head.
#[must_use]
pub fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let head_len = max / 2;
    let tail_len = max - head_len;
    let mut head_end = head_len;
    while !text.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = text.len() - tail_len;
    while !text.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    format!(
        "{}\n... [{} bytes truncated] ...\n{}",
        &text[..head_end],
        tail_start - head_end,
        &text[tail_start..]
    )
}

#[cfg(test)]
mod test;
