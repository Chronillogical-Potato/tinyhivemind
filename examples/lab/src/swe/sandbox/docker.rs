//! `docker exec` into a named running container.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::{Exec, ExecOutput};

/// Runs commands in one container, as bash, with stderr merged into stdout.
pub struct DockerExec {
    container: String,
}

impl DockerExec {
    /// Target the container named `container`.
    #[must_use]
    pub fn new(container: &str) -> Self {
        Self {
            container: container.to_owned(),
        }
    }
}

impl Exec for DockerExec {
    fn exec(&self, cmd: &str, timeout: Duration) -> Result<ExecOutput, String> {
        let secs = timeout.as_secs().max(1).to_string();
        // `timeout` inside the container kills the real process; the host-side
        // deadline below only covers a wedged docker client.
        let mut child = Command::new("docker")
            .args(["exec", "-i", &self.container])
            .args(["timeout", "-k", "2", &secs, "bash", "-c"])
            .arg(format!("exec 2>&1\n{cmd}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not run docker: {error}"))?;
        let mut stdout = child.stdout.take().ok_or("no stdout pipe")?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        let deadline = Instant::now() + timeout + Duration::from_secs(10);
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(format!("docker wait failed: {error}")),
            }
        };
        let bytes = reader.join().unwrap_or_default();
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        let exit = match status {
            Some(status) => status.code().unwrap_or(-1),
            None => 124,
        };
        if exit == 124 || exit == 137 {
            text.push_str(&format!("\n[timed out after {secs}s]"));
        }
        Ok(ExecOutput { stdout: text, exit })
    }
}
