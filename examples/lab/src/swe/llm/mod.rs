//! A metered chat-completions client with tool calling.
//!
//! [`Llm`] wraps any [`Chat`] transport with the run's [`Meter`]: it refuses a
//! call once a cap is reached, retries a failed call once, and records the
//! provider's reported usage against the calling seat. [`CurlChat`] is the
//! real transport: it shells out to `curl` and hands the whole request,
//! API key included, over curl's stdin as a config file, so the key never
//! appears in a process argument list. The key is read from the environment by
//! the caller and is never printed or logged.

mod wire;

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use super::meter::{Abort, Meter};
pub use wire::{Completion, ToolUse, config_escape, parse_response, request_body, tool_result};

/// A transport that turns one conversation into one reply.
pub trait Chat: Send + Sync {
    /// Send the conversation and tools; return the raw response body.
    ///
    /// # Errors
    ///
    /// Returns a message for a transport or HTTP failure.
    fn send(&self, body: &Value) -> Result<Value, String>;
}

/// The real transport: `curl` against an OpenAI-compatible endpoint.
pub struct CurlChat {
    url: String,
    key: String,
    timeout_secs: u64,
}

impl CurlChat {
    /// A transport for `api_base` (without `/chat/completions`).
    ///
    /// An empty `key` sends no `Authorization` header, which is what a local
    /// mock server wants.
    #[must_use]
    pub fn new(api_base: &str, key: String, timeout_secs: u64) -> Self {
        Self {
            url: format!("{}/chat/completions", api_base.trim_end_matches('/')),
            key,
            timeout_secs,
        }
    }

    fn script(&self, body: &Value) -> String {
        let mut script = format!(
            "url = \"{}\"\nrequest = \"POST\"\nsilent\nshow-error\nheader = \"Content-Type: application/json\"\n",
            config_escape(&self.url)
        );
        if !self.key.is_empty() {
            script.push_str(&format!(
                "header = \"Authorization: Bearer {}\"\n",
                config_escape(&self.key)
            ));
        }
        script.push_str(&format!(
            "data-binary = \"{}\"\nmax-time = {}\nwrite-out = \"\\n%{{http_code}}\"\n",
            config_escape(&body.to_string()),
            self.timeout_secs
        ));
        script
    }
}

impl Chat for CurlChat {
    fn send(&self, body: &Value) -> Result<Value, String> {
        let mut child = Command::new("curl")
            .args(["--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not run curl: {error}"))?;
        child
            .stdin
            .take()
            .ok_or("failed to open curl input")?
            .write_all(self.script(body).as_bytes())
            .map_err(|error| format!("failed to send curl config: {error}"))?;
        let out = child
            .wait_with_output()
            .map_err(|error| format!("curl failed: {error}"))?;
        if !out.status.success() {
            return Err(format!(
                "curl exited {}: {}",
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (payload, status) = text.rsplit_once('\n').unwrap_or((&text, ""));
        let parsed: Result<Value, _> = serde_json::from_str(payload);
        match (status.trim(), parsed) {
            (_, Ok(value)) if value.get("error").is_none() => Ok(value),
            ("200", Ok(value)) => Ok(value),
            (code, Ok(value)) => Err(format!("http {code}: {}", value["error"])),
            (code, Err(error)) => Err(format!("http {code}: unreadable body ({error})")),
        }
    }
}

/// The metered client every seat shares.
pub struct Llm {
    chat: Box<dyn Chat>,
    model: String,
    meter: Meter,
    retry_pause: Duration,
}

impl Llm {
    /// Wrap a transport.
    #[must_use]
    pub fn new(chat: Box<dyn Chat>, model: &str, meter: Meter) -> Self {
        Self {
            chat,
            model: model.to_owned(),
            meter,
            retry_pause: Duration::from_millis(800),
        }
    }

    /// Skip the pause between a failed call and its retry (tests).
    #[must_use]
    pub fn without_retry_pause(mut self) -> Self {
        self.retry_pause = Duration::ZERO;
        self
    }

    /// The model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The shared meter.
    #[must_use]
    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    /// One metered model call on behalf of `seat`.
    ///
    /// # Errors
    ///
    /// [`Abort::TokenCap`] / [`Abort::MaxTurns`] before the call when a cap is
    /// reached, or [`Abort::Llm`] when the call fails twice.
    pub fn complete(
        &self,
        seat: &str,
        messages: &[Value],
        tools: &[Value],
    ) -> Result<Completion, Abort> {
        self.meter.begin_call()?;
        let body = request_body(&self.model, messages, tools);
        let mut attempt = self.chat.send(&body).and_then(|v| parse_response(&v));
        if attempt.is_err() {
            std::thread::sleep(self.retry_pause);
            attempt = self.chat.send(&body).and_then(|v| parse_response(&v));
        }
        let completion = attempt.map_err(Abort::Llm)?;
        self.meter
            .record(seat, completion.input_tokens, completion.output_tokens);
        Ok(completion)
    }
}

#[cfg(test)]
mod test;
