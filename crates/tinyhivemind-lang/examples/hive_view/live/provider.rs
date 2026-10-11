//! The OpenRouter transport: one chat completion per call, through `curl`.
//!
//! The pattern is `examples/live_self_edit.rs`'s, tightened: the key is read
//! from `OPENROUTER_API_KEY`, checked to be safe inside a curl config line,
//! and handed to curl as a `--config -` header on **stdin**, so it never
//! appears in argv (and so in `ps`), in a request file, or in a log. Every
//! request body and response is kept under `target/hive-live/<run>/`.
//!
//! Web access is denied by construction: [`request_body`] builds the only
//! body this module sends, and it carries no `tools`, `tool_choice`,
//! `plugins` or `web_search_options`; [`check_model`] refuses an `:online`
//! variant or a search model before any call is made.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

use crate::Result;

/// The one endpoint a live run calls.
pub(crate) const ENDPOINT: &str = "https://openrouter.ai/api/v1/chat/completions";

/// The model a live run uses when neither `--model` nor `OPENROUTER_MODEL`
/// names one: the one `live_self_edit.rs` uses, cheap and capable.
pub(crate) const DEFAULT_MODEL: &str = "openai/gpt-oss-120b:nitro";

/// Body keys that would give a model tools or the web. None is ever sent.
pub(crate) const FORBIDDEN_KEYS: [&str; 5] = [
    "tools",
    "tool_choice",
    "plugins",
    "web_search_options",
    "functions",
];

/// Refuse a model id that is malformed or would reach the web.
///
/// The id must be `vendor/name[:variant]` in a conservative charset. An
/// `:online` variant (OpenRouter's web-search suffix), and any id naming a
/// search model (`online`, `search`, `sonar`, `web`), is refused before a
/// single call is made.
///
/// # Errors
///
/// Fails with the reason the id is refused.
pub(crate) fn check_model(id: &str) -> Result<()> {
    let charset = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':');
    if id.is_empty() || id.len() > 100 || !id.chars().all(charset) {
        return Err(format!("--model {id:?} is not a model id (vendor/name[:variant])").into());
    }
    if !id.starts_with(|c: char| c.is_ascii_alphanumeric()) || id.matches(':').count() > 1 {
        return Err(format!("--model {id:?} is not a model id (vendor/name[:variant])").into());
    }
    let lower = id.to_ascii_lowercase();
    for web in ["online", "search", "sonar", "web"] {
        if lower.contains(web) {
            return Err(format!(
                "--model {id:?} is refused: live runs deny web access, and {web:?} names a web-search variant"
            )
            .into());
        }
    }
    Ok(())
}

/// The request body for one call. Nothing else is ever sent.
pub(crate) fn request_body(
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    seed: u64,
) -> Value {
    json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "max_tokens": max_tokens,
        "temperature": 0.7,
        "seed": seed,
        // Spend reasoning tokens sparingly and keep them out of the reply.
        "reasoning": {"effort": "low", "exclude": true},
        // Ask OpenRouter to report each call's cost alongside its tokens.
        "usage": {"include": true},
    })
}

/// Tokens and cost, as OpenRouter reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Usage {
    pub(crate) calls: u64,
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
    pub(crate) total_tokens: u64,
    pub(crate) cost_usd: f64,
}

impl Usage {
    fn add(&mut self, wire: &Value) {
        let field = |name: &str| wire[name].as_u64().unwrap_or(0);
        self.calls += 1;
        self.prompt_tokens += field("prompt_tokens");
        self.completion_tokens += field("completion_tokens");
        self.total_tokens += field("total_tokens");
        self.cost_usd += wire["cost"].as_f64().unwrap_or(0.0);
    }
}

/// One call's result.
#[derive(Clone, Debug)]
pub(crate) enum Reply {
    /// The model's text (possibly empty).
    Text(String),
    /// A call that failed in a way the run can survive (a timeout, a 5xx, a
    /// malformed body); the message is already redacted.
    Failed(String),
}

/// The live provider: key, model, and the run's artifact directory.
#[derive(Debug)]
pub(crate) struct Provider {
    key: String,
    pub(crate) model: String,
    pub(crate) max_tokens: u32,
    pub(crate) dir: PathBuf,
    issued: AtomicU64,
    usage: Mutex<Usage>,
}

/// `target/hive-live/` under the workspace this example was built in.
pub(crate) fn artifacts_root() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::canonicalize(&workspace)
        .unwrap_or(workspace)
        .join("target/hive-live")
}

impl Provider {
    /// Read the key from the environment and create the run directory.
    ///
    /// # Errors
    ///
    /// Fails when the key is missing or unsafe to place in a curl config
    /// line, when the model is refused, or the directory cannot be made.
    pub(crate) fn from_env(model: &str, max_tokens: u32, run: &str) -> Result<Self> {
        check_model(model)?;
        let key = std::env::var("OPENROUTER_API_KEY")
            .map_err(|_| "--live needs OPENROUTER_API_KEY in the environment")?;
        if key.is_empty() || key.bytes().any(|b| b < 0x21 || b > 0x7e || b == b'\\' || b == b'"')
        {
            return Err("OPENROUTER_API_KEY is not a printable token".into());
        }
        let dir = artifacts_root().join(run);
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            key,
            model: model.to_owned(),
            max_tokens,
            dir,
            issued: AtomicU64::new(0),
            usage: Mutex::new(Usage::default()),
        })
    }

    /// Calls issued so far.
    pub(crate) fn issued(&self) -> u64 {
        self.issued.load(Ordering::SeqCst)
    }

    /// Usage so far.
    pub(crate) fn usage(&self) -> Usage {
        self.usage.lock().map(|usage| *usage).unwrap_or_default()
    }

    /// Make one call. Returns the call's number and its reply.
    ///
    /// # Errors
    ///
    /// Fails — ending the run — on an authentication, payment or permission
    /// refusal (HTTP 401, 402, 403), or when curl cannot be started.
    pub(crate) fn complete(&self, system: &str, user: &str, seed: u64) -> Result<(u64, Reply)> {
        let number = self.issued.fetch_add(1, Ordering::SeqCst) + 1;
        let body = request_body(&self.model, system, user, self.max_tokens, seed);
        if let Some(key) = FORBIDDEN_KEYS.iter().find(|key| body.get(**key).is_some()) {
            return Err(format!("refusing to send a request carrying {key}").into());
        }
        let request = self.dir.join(format!("call-{number:04}.request.json"));
        std::fs::write(&request, serde_json::to_vec_pretty(&body)?)?;
        let mut child = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--max-time",
                "180",
                "--retry",
                "2",
                "--retry-delay",
                "3",
                "--config",
                "-",
                "--header",
                "Content-Type: application/json",
                "--write-out",
                "\n%{http_code}",
                "--data-binary",
            ])
            .arg(format!("@{}", request.display()))
            .arg(ENDPOINT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        {
            let mut stdin = child.stdin.take().ok_or("curl stdin unavailable")?;
            // The key travels on stdin only: never argv, never a file.
            writeln!(stdin, "header = \"Authorization: Bearer {}\"", self.key)?;
        }
        let output = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).replace(&self.key, "[redacted]");
        let (payload, status) = stdout
            .rsplit_once('\n')
            .map_or((stdout.as_str(), ""), |(body, code)| (body, code.trim()));
        let stderr: String = String::from_utf8_lossy(&output.stderr)
            .replace(&self.key, "[redacted]")
            .trim()
            .chars()
            .take(240)
            .collect();
        let status: u16 = status.parse().unwrap_or(0);
        if !output.status.success() || status != 200 {
            std::fs::write(
                self.dir.join(format!("call-{number:04}.error.txt")),
                format!("http {status}\n{payload}\n{stderr}\n"),
            )?;
            let message = format!(
                "call {number} failed (http {status}): {}",
                if payload.trim().is_empty() {
                    stderr.as_str()
                } else {
                    payload.trim()
                }
                .chars()
                .take(300)
                .collect::<String>()
            );
            if matches!(status, 401..=403) {
                return Err(message.into());
            }
            return Ok((number, Reply::Failed(message)));
        }
        let wire: Value = match serde_json::from_str(payload) {
            Ok(wire) => wire,
            Err(error) => return Ok((number, Reply::Failed(format!("call {number}: {error}")))),
        };
        std::fs::write(
            self.dir.join(format!("call-{number:04}.response.json")),
            serde_json::to_vec_pretty(&wire)?,
        )?;
        if let Ok(mut usage) = self.usage.lock() {
            usage.add(&wire["usage"]);
        }
        if let Some(error) = wire.get("error") {
            return Ok((number, Reply::Failed(format!("call {number}: {error}"))));
        }
        let text = wire["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        Ok((number, Reply::Text(text)))
    }
}
