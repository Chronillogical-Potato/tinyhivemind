//! Drive one seat as a standalone participant over a CLI subprocess.
//!
//! [`LiveAgent`] is the plain, non-federated case: one process per turn,
//! answering the shared brief with no channel to any other desk. [`super::desk`]
//! wraps this to add the federation's cross-channel move; everything about
//! running the process and parsing its answer is owned here so the two seats
//! parse identically.

use std::process::Command;

use tinyhivemind_core::hive::{HiveTurn, QuorumPolicy, SessionMessage};

use super::{AgentPrompt, timeout_binary};
use crate::run::Participant;

/// A participant backed by an external agent command.
pub(crate) struct LiveAgent {
    prompt: AgentPrompt,
    program: String,
    args: Vec<String>,
    /// Per-turn timeout in seconds, applied through `timeout`/`gtimeout` when
    /// one is on `PATH`.
    timeout_secs: u64,
}

impl LiveAgent {
    /// Build a participant from a command line such as `opencode run`.
    ///
    /// The prompt is appended as the final argument.
    pub(crate) fn new(
        id: &str,
        role: &str,
        command: &str,
        quorum: QuorumPolicy,
        private: String,
        timeout_secs: u64,
    ) -> Option<Self> {
        let (program, args) = split_command(command)?;
        Some(Self {
            prompt: AgentPrompt::new(id, role, quorum, private),
            program,
            args,
            timeout_secs,
        })
    }

    /// What only this member knows, as a block for a prompt.
    pub(crate) fn private(&self) -> &str {
        self.prompt.private()
    }

    /// Render exactly what this turn is allowed to see.
    pub(crate) fn prompt(&self, turn: &HiveTurn, visible: &[SessionMessage]) -> String {
        self.prompt.prompt(turn, visible)
    }

    /// The same, with one extra block of moves offered alongside the protocol.
    pub(crate) fn prompt_with(
        &self,
        turn: &HiveTurn,
        visible: &[SessionMessage],
        extra: &str,
    ) -> String {
        self.prompt.prompt_with(turn, visible, extra)
    }

    /// Run one prompt through the agent process and take its one line.
    ///
    /// One retry on a non-zero exit or an empty answer: a live process
    /// occasionally drops a turn for reasons that have nothing to do with the
    /// protocol — a cold start, a rate limit, a flaky network call — and
    /// spending the episode's whole turn on that is a worse failure than
    /// spending one extra process on a retry.
    ///
    /// # Errors
    ///
    /// Returns the second attempt's failure: the process could not be
    /// started, or exited non-zero.
    pub(crate) fn line(&self, prompt: &str) -> Result<String, String> {
        match self.attempt(prompt) {
            Ok(answer) if answer != "(no answer)" => Ok(answer),
            _ => self.attempt(prompt),
        }
    }

    /// Run the agent process once and take its one line, with no retry.
    fn attempt(&self, prompt: &str) -> Result<String, String> {
        let mut command = timeout_binary().map_or_else(
            || Command::new(&self.program),
            |bin| {
                let mut wrapped = Command::new(bin);
                wrapped
                    .arg(self.timeout_secs.to_string())
                    .arg(&self.program);
                wrapped
            },
        );
        let output = command
            .args(&self.args)
            .arg(prompt)
            .output()
            .map_err(|error| format!("could not run {}: {error}", self.program))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let head = stderr.lines().take(3).collect::<Vec<_>>().join(" | ");
            return Err(format!(
                "{} exited with {}: {head}",
                self.program, output.status,
            ));
        }
        Ok(marker_line(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// The one line a seat's answer contributes to the transcript.
///
/// Colour and banners are stripped first, then the marker line is taken if
/// the agent wrapped it in prose. A turn that deposits no trace is still a
/// legal turn, so prose falls through to the first thing the agent actually
/// said.
///
/// Shared with [`crate::http`] rather than reimplemented there: an HTTP seat
/// and a CLI seat have to parse identically or a room that mixes the two is
/// not one room. The escape stripping is inert on a JSON body that never
/// carried an escape, so applying it to both costs nothing.
pub(crate) fn marker_line(text: &str) -> String {
    let text = plain(text);
    let marker = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('!') || line.starts_with('@'));
    marker
        .or_else(|| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('>'))
        })
        .unwrap_or("(no answer)")
        .to_owned()
}

impl Participant for LiveAgent {
    fn id(&self) -> &str {
        self.prompt.id()
    }

    fn speak(&mut self, turn: &HiveTurn, visible: &[SessionMessage]) -> Result<String, String> {
        self.line(&self.prompt(turn, visible))
    }
}

/// Strip ANSI escape sequences from a CLI's output.
///
/// Real agent CLIs colour what they print and draw a banner around it. A
/// marker line preceded by a colour reset is still a marker line, and the
/// grammar reads the text rather than the terminal.
pub(crate) fn plain(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        // CSI sequences end at their final byte in `@`..=`~`; anything else
        // after the escape is a two-character sequence.
        if characters.next() == Some('[') {
            for byte in characters.by_ref() {
                if ('@'..='~').contains(&byte) {
                    break;
                }
            }
        }
    }
    plain
}

/// Split an agent command line into its program and leading arguments.
pub(crate) fn split_command(command: &str) -> Option<(String, Vec<String>)> {
    let mut words = command.split_whitespace().map(str::to_owned);
    let program = words.next()?;
    Some((program, words.collect()))
}
