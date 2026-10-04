//! Types of the working-memory port.

use serde::{Deserialize, Serialize};

/// Who may recall an entry.
///
/// The hive states the intent; the host's engine decides how to honor it. An
/// engine with no notion of scope may treat both the same.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    /// Shared by every seat of the hive.
    #[default]
    Hive,
    /// Private to the seat that wrote it.
    Seat,
}

/// One thing a seat asks the host to remember.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryNote {
    /// The seat writing the note.
    pub author: String,
    /// Who may recall it.
    #[serde(default)]
    pub scope: MemoryScope,
    /// What to remember, as prose.
    pub text: String,
}

/// A question put to the host's memory on behalf of one seat.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryQuery {
    /// The seat asking. Entries private to other seats must not be returned.
    pub seat: String,
    /// What the seat wants to remember; empty asks for the most relevant
    /// entries overall.
    pub query: String,
    /// The most entries wanted, at most [`MEMORY_LIMIT`](super::MEMORY_LIMIT).
    pub limit: usize,
}

/// One remembered entry, as a host's engine returns it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryEntry {
    /// The engine's own handle, usable with `forget`.
    pub id: String,
    /// The seat that wrote it.
    pub author: String,
    /// Who may recall it.
    #[serde(default)]
    pub scope: MemoryScope,
    /// The remembered text.
    pub text: String,
}
