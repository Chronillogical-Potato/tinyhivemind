//! JSON-lines RPC over our own stdout and stdin.
//!
//! The Harbor agent is a Python process that owns the task environment. It
//! spawns this binary with `--stdio-rpc`; every command a seat runs becomes one
//! request line on our stdout, and the agent answers on our stdin:
//!
//! ```text
//! -> {"id":1,"exec":"ls /app","timeout":120}
//! <- {"id":1,"stdout":"...","exit":0}
//! ```
//!
//! Seats run concurrently, so replies are demultiplexed by `id` on a reader
//! thread and each caller waits on its own channel. In this mode nothing else
//! may be printed on stdout; diagnostics go to stderr.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::mpsc::{self, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::{Exec, ExecOutput};

/// One decoded reply line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reply {
    /// The request id it answers.
    pub id: u64,
    /// Output of the command.
    pub stdout: String,
    /// Exit code.
    pub exit: i32,
}

/// Encode one request line (without the newline).
#[must_use]
pub fn encode_request(id: u64, cmd: &str, timeout: Duration) -> String {
    json!({ "id": id, "exec": cmd, "timeout": timeout.as_secs() }).to_string()
}

/// Decode one reply line; `None` for anything that is not a reply.
#[must_use]
pub fn decode_reply(line: &str) -> Option<Reply> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    Some(Reply {
        id: value.get("id")?.as_u64()?,
        stdout: value
            .get("stdout")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        exit: i32::try_from(value.get("exit").and_then(Value::as_i64).unwrap_or(-1)).unwrap_or(-1),
    })
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Reply>>>>;

/// Sends requests to a writer and collects replies from a reader.
pub struct StdioExec {
    out: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    closed: Arc<AtomicBool>,
    next: Mutex<u64>,
}

impl StdioExec {
    /// Speak the protocol over `input` (replies) and `output` (requests).
    ///
    /// Spawns the reader thread; it ends when `input` reaches end of file,
    /// which fails every call still waiting.
    pub fn new<R, W>(input: R, output: W) -> Self
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        let pending: Pending = Arc::default();
        let waiting = Arc::clone(&pending);
        let closed = Arc::new(AtomicBool::new(false));
        let ended = Arc::clone(&closed);
        std::thread::spawn(move || {
            for line in input.lines() {
                let Ok(line) = line else { break };
                let Some(reply) = decode_reply(&line) else {
                    continue;
                };
                let sender = waiting.lock().ok().and_then(|mut map| map.remove(&reply.id));
                if let Some(sender) = sender {
                    let _ = sender.send(reply);
                }
            }
            ended.store(true, Ordering::SeqCst);
            if let Ok(mut map) = waiting.lock() {
                map.clear();
            }
        });
        Self {
            out: Mutex::new(Box::new(output)),
            pending,
            closed,
            next: Mutex::new(0),
        }
    }
}

impl Exec for StdioExec {
    fn exec(&self, cmd: &str, timeout: Duration) -> Result<ExecOutput, String> {
        let id = {
            let mut next = self.next.lock().map_err(|_| "rpc id lock poisoned")?;
            *next += 1;
            *next
        };
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "rpc pending lock poisoned")?
            .insert(id, tx);
        if self.closed.load(Ordering::SeqCst) {
            return Err("rpc peer closed".to_owned());
        }
        {
            let mut out = self.out.lock().map_err(|_| "rpc writer lock poisoned")?;
            writeln!(out, "{}", encode_request(id, cmd, timeout))
                .and_then(|()| out.flush())
                .map_err(|error| format!("rpc write failed: {error}"))?;
        }
        // The agent enforces `timeout`; the grace only covers a dead agent.
        match rx.recv_timeout(timeout + Duration::from_secs(60)) {
            Ok(reply) => Ok(ExecOutput {
                stdout: reply.stdout,
                exit: reply.exit,
            }),
            Err(_) => Err("rpc peer closed or did not answer".to_owned()),
        }
    }
}
