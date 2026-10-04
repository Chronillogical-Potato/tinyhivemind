//! The seat-facing memory port and the values that cross it.
//!
//! A seat never sees an engine. It asks a [`SeatMemory`] for a pack at one of
//! three [`Moment`]s and hands it what it did as [`Remembered`]; every call
//! comes back with a [`Report`] the seat turns into a `memory` mark. A core
//! `Recall`/`Remember` port can be implemented by a thin adapter over this
//! trait, or this trait over it.

/// When a seat asks for memory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Moment {
    /// The seat's session is new: everything relevant to `focus`, the seat's
    /// own earlier turns included (a `fresh` session relies on them).
    SessionStart {
        /// What the seat is about to do: the task, or the wake reason.
        focus: String,
    },
    /// A persistent session resumes: only what teammates stored since this
    /// seat last recalled, since the session already holds its own work.
    Rejoin {
        /// Why the seat was woken.
        focus: String,
    },
    /// Messages were just dropped from the session by compaction.
    Compaction {
        /// The dropped messages' text, oldest first.
        dropped: Vec<String>,
        /// What the seat is doing now.
        focus: String,
    },
}

impl Moment {
    /// The name used in marks.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::SessionStart { .. } => "session_start",
            Self::Rejoin { .. } => "rejoin",
            Self::Compaction { .. } => "compaction",
        }
    }
}

/// One command a seat ran, as memory keeps it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgerEntry {
    /// The command, clipped.
    pub cmd: String,
    /// Its exit code; `None` when it could not run or was refused.
    pub exit: Option<i32>,
    /// One clipped line of what it printed or why it failed.
    pub outcome: String,
}

impl LedgerEntry {
    /// Whether this was a failed attempt.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.exit != Some(0)
    }

    /// One bullet: `[ok exit 0] `cmd` -> outcome`, or `[FAILED ...]`.
    #[must_use]
    pub fn render(&self) -> String {
        let status = match self.exit {
            Some(0) => "ok exit 0".to_owned(),
            Some(code) => format!("FAILED attempt, exit {code}"),
            None => "FAILED attempt, did not run".to_owned(),
        };
        format!("- [{status}] `{}` -> {}", self.cmd, self.outcome)
    }
}

/// What a seat hands memory at the end of an activation (or before a
/// compaction): its last words and the commands since the previous hand-off.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Remembered {
    /// The seat's final text, or a note of what it was doing.
    pub text: String,
    /// Commands run since the last hand-off.
    pub ledger: Vec<LedgerEntry>,
}

impl Remembered {
    /// Whether there is nothing to store.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.ledger.is_empty()
    }

    /// The stored turn's text: the words, then one bullet per command.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = self.text.trim().to_owned();
        if !self.ledger.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str("Commands:\n");
            let lines: Vec<String> = self.ledger.iter().map(LedgerEntry::render).collect();
            out.push_str(&lines.join("\n"));
        }
        out
    }
}

/// What one memory call did, for the `memory` mark.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report {
    /// `recall`, `remember` or `background`.
    pub op: &'static str,
    /// The [`Moment::name`], or `activation` for a hand-off.
    pub moment: &'static str,
    /// Characters recalled or stored.
    pub chars: usize,
    /// Items cited by the pack, or commands stored.
    pub items: usize,
    /// Wall time of the call.
    pub latency_ms: u64,
    /// Why it produced nothing, when it failed or timed out.
    pub error: Option<String>,
}

impl Report {
    /// The mark's detail line for `seat`.
    #[must_use]
    pub fn detail(&self, seat: &str) -> String {
        let mut line = format!(
            "{seat}: {} {} chars={} items={} latency_ms={}",
            self.op, self.moment, self.chars, self.items, self.latency_ms
        );
        if let Some(error) = &self.error {
            line.push_str(&format!(" error={error}"));
        }
        line
    }
}

/// A recall's pack (already framed and clipped) and its report.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recalled {
    /// The text to put in front of the model; `None` when nothing was found
    /// or the call failed.
    pub pack: Option<String>,
    /// What the call did.
    pub report: Report,
}

/// Memory as a seat sees it. Implementations never fail the seat: errors and
/// timeouts come back as an empty pack with [`Report::error`] set.
pub trait SeatMemory: Sync {
    /// A pack for `seat` at `moment`.
    fn recall(&self, seat: &str, moment: &Moment) -> Recalled;

    /// Store what `seat` did.
    fn remember(&self, seat: &str, what: &Remembered) -> Report;

    /// Wait (bounded) for background work and report it; called once at the
    /// end of a run.
    fn finish(&self) -> Vec<Report> {
        Vec::new()
    }
}
