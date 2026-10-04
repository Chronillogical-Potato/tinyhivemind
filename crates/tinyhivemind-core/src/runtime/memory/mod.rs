//! The working-memory port: what a hive asks of whatever remembers for it.
//!
//! A seat loses its context between activations. Something has to carry
//! observations, claims and dead ends across them, and that something is a
//! plain markdown file for one host, a vector store for another, a memory
//! service for a third. This module is deliberately none of those. It states
//! the narrow question a hive asks — *recall*, *record*, *forget* — as the
//! [`WorkingMemory`] port, bounds what crosses it, and renders what comes
//! back as one [`BriefingNote`] a turn opens with.
//!
//! Three rules keep it an adapter rather than an engine:
//!
//! 1. **The host owns storage and retrieval.** Nothing here names a format,
//!    an index, or a ranking model. A recall returns entries in the order the
//!    engine thinks best, and the fold below trusts that order.
//! 2. **Everything crossing the port is bounded.** A note is at most
//!    [`MEMORY_NOTE_CHARS`] characters, a recall at most [`MEMORY_LIMIT`]
//!    entries, and the rendered note is cut to a character budget, so a
//!    generous engine cannot spend a seat's window.
//! 3. **A missing memory costs recall, nothing else.** Like the digester, a
//!    port that is absent, slow or wrong degrades the briefing; it does not
//!    break a turn.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_core::runtime::{MemoryEntry, MemoryScope, hive_memory_note};
//!
//! let entries = vec![MemoryEntry {
//!     id: "m1".into(),
//!     author: "solver".into(),
//!     scope: MemoryScope::Hive,
//!     text: "the eval harness needs --no-cache; plain runs hang".into(),
//! }];
//! let note = hive_memory_note(&entries, 1_000).expect("one entry renders");
//! assert_eq!(note.heading, "Working memory");
//! assert!(note.lines[0].contains("solver"));
//! ```

mod types;

pub use types::{MemoryEntry, MemoryNote, MemoryQuery, MemoryScope};

use crate::runtime::{BoxError, BriefingNote, Error, Result};
use std::{future::Future, pin::Pin};

/// The most entries one recall may return or one rendered note may carry.
pub const MEMORY_LIMIT: usize = 12;

/// The longest note, in characters, the port accepts.
pub const MEMORY_NOTE_CHARS: usize = 1_000;

/// The boxed, executor-neutral future returned by [`WorkingMemory`].
pub type MemoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = std::result::Result<T, BoxError>> + Send + 'a>>;

/// Whatever remembers for a hive, behind a host's own engine.
///
/// The trait is object-safe. A host implements it over a file, a database or
/// a service; the hive reaches it through the memory tools and the briefing.
pub trait WorkingMemory: Send + Sync {
    /// Entries relevant to `query`, best first, visible to `query.seat`.
    fn recall<'a>(&'a self, query: &'a MemoryQuery) -> MemoryFuture<'a, Vec<MemoryEntry>>;

    /// Remember `note` and return the entry the engine made of it.
    fn record<'a>(&'a self, note: &'a MemoryNote) -> MemoryFuture<'a, MemoryEntry>;

    /// Drop the entry `id` on behalf of `seat`. Forgetting an entry the seat
    /// may not see, or that is already gone, is the engine's call to refuse or
    /// ignore.
    fn forget<'a>(&'a self, seat: &'a str, id: &'a str) -> MemoryFuture<'a, ()>;
}

/// Check a note before it crosses the port.
///
/// # Errors
///
/// Returns [`Error::MemoryNoteEmpty`] for a blank note and
/// [`Error::MemoryNoteTooLong`] past [`MEMORY_NOTE_CHARS`].
pub fn validate_note(note: &MemoryNote) -> Result<()> {
    let actual = note.text.chars().count();
    if note.text.trim().is_empty() {
        return Err(Error::MemoryNoteEmpty);
    }
    if actual > MEMORY_NOTE_CHARS {
        return Err(Error::MemoryNoteTooLong {
            limit: MEMORY_NOTE_CHARS,
            actual,
        });
    }
    Ok(())
}

/// Recall for a seat, bounded and validated.
///
/// `limit` is clamped to `1..=`[`MEMORY_LIMIT`], and entries beyond it are
/// dropped even when an engine ignores the limit it was given.
///
/// # Errors
///
/// Returns [`Error::Memory`] when the host's engine fails.
pub async fn recall(
    memory: &(dyn WorkingMemory + '_),
    seat: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<MemoryEntry>> {
    let request = MemoryQuery {
        seat: seat.to_owned(),
        query: query.to_owned(),
        limit: limit.clamp(1, MEMORY_LIMIT),
    };
    let mut entries = memory
        .recall(&request)
        .await
        .map_err(|source| Error::Memory { source })?;
    entries.truncate(request.limit);
    Ok(entries)
}

/// Record a note after validating it.
///
/// # Errors
///
/// Returns a validation error from [`validate_note`], or [`Error::Memory`]
/// when the host's engine fails.
pub async fn record(memory: &(dyn WorkingMemory + '_), note: &MemoryNote) -> Result<MemoryEntry> {
    validate_note(note)?;
    memory
        .record(note)
        .await
        .map_err(|source| Error::Memory { source })
}

/// Render recalled entries as the one briefing note a turn opens with.
///
/// Entries keep the engine's order. Each is one line, `[author] text`, and the
/// note stops before the line that would pass `budget_chars`, so the briefing
/// spends a bounded amount whatever the engine returned. `None` when there is
/// nothing to say, or the budget fits no entry.
#[must_use]
pub fn hive_memory_note(entries: &[MemoryEntry], budget_chars: usize) -> Option<BriefingNote> {
    let mut spent = 0_usize;
    let mut lines = Vec::new();
    for entry in entries.iter().take(MEMORY_LIMIT) {
        let text = entry.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = format!("[{}] {text}", entry.author);
        spent += line.chars().count();
        if spent > budget_chars {
            break;
        }
        lines.push(line);
    }
    (!lines.is_empty()).then(|| BriefingNote {
        heading: "Working memory".to_owned(),
        lines,
    })
}

#[cfg(test)]
mod test;
