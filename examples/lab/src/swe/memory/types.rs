//! What a seat hands the memory port, and the lab's extension of core's ports.
//!
//! The ports themselves are core's: [`Recall`] and [`Remember`] in
//! `tinyhivemind_core::runtime::recall`. A seat records each command it runs
//! as a [`LedgerEntry`] and turns the ledger into core [`MemoryEntry`] values
//! when it remembers: a command that exited 0 is an
//! [`EntryKind::Observation`], anything else an [`EntryKind::FailedAttempt`].

use std::time::Duration;

use tinyhivemind_core::runtime::{EntryKind, MemoryEntry, Recall, Remember};

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

    /// The core entry: an observation, or a failed attempt with its exit.
    #[must_use]
    pub fn to_entry(&self) -> MemoryEntry {
        let (kind, status) = match self.exit {
            Some(0) => (EntryKind::Observation, "exit 0".to_owned()),
            Some(code) => (EntryKind::FailedAttempt, format!("failed, exit {code}")),
            None => (EntryKind::FailedAttempt, "failed, did not run".to_owned()),
        };
        MemoryEntry {
            kind,
            text: format!("`{}` ({status}) -> {}", self.cmd, self.outcome),
        }
    }
}

/// The label an entry is stored under, so a later reader sees what it was.
#[must_use]
pub const fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Observation => "observation",
        EntryKind::FailedAttempt => "FAILED attempt",
        EntryKind::Outcome => "outcome",
        EntryKind::Note => "note",
    }
}

/// Core's two memory ports plus what the lab's seats need around them: the
/// run's namespace, the recall budget, and a bounded end-of-run drain.
pub trait SeatMemory: Recall + Remember {
    /// The memory namespace of this run, the `conversation` of every request.
    fn conversation(&self) -> String;

    /// The character budget a recalled block is framed within.
    fn budget_chars(&self) -> usize;

    /// Wait (bounded) for background work and describe it, one line per
    /// job; called once at the end of a run.
    fn finish(&self) -> Vec<String> {
        Vec::new()
    }
}

/// How long each kind of call may take before it is abandoned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timeouts {
    /// A recall at any moment.
    pub recall: Duration,
    /// Storing one activation, until it is indexed.
    pub remember: Duration,
    /// Waiting for every outstanding belief build at the end of the run.
    pub finish: Duration,
}

impl Timeouts {
    /// Recall and remember 4 s each (a compaction recall measured ~1.4 s
    /// against CortexDB), 20 s for background work at the end.
    pub const DEFAULT: Self = Self {
        recall: Duration::from_secs(4),
        remember: Duration::from_secs(4),
        finish: Duration::from_secs(20),
    };
}
