//! Stable records for the host memory port: what a recall asks for, what a
//! remember writes, where a seat's view of the desk stands, and what a session
//! initialized with recall hands back.

use crate::runtime::{BriefingNote, Error, Sequence, SessionInitialization, SessionMessage};
use serde::{Deserialize, Serialize};

/// The moment in a seat's life at which the host recalls memory for it.
///
/// A seat keeps one session for its whole life, so recall happens only where
/// that session is missing something it cannot get from the desk: when it is
/// first opened, when the seat rejoins after peers have worked without it, and
/// when compaction has just erased part of it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecallMoment {
    /// The seat's session is being opened for the first time in this run.
    SessionStart,
    /// The seat is rejoining a session it already holds. Only memory other
    /// seats wrote since its last activation is wanted.
    Rejoin,
    /// The host compacted the seat's session; recall what that dropped.
    Compaction {
        /// Short descriptions of what compaction removed, used as the query.
        dropped: Vec<String>,
    },
}

impl RecallMoment {
    /// The stable `snake_case` name of this moment, as its wire tag spells it.
    ///
    /// This is the string [`TraceEvent::Recalled`](crate::telemetry::TraceEvent::Recalled)
    /// carries in its `moment` field.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::Rejoin => "rejoin",
            Self::Compaction { .. } => "compaction",
        }
    }
}

/// One recall a host memory store answers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RecallRequest {
    /// The seat (agent id) the memory is recalled for.
    pub seat: String,
    /// The host's memory namespace for this conversation.
    ///
    /// A host scopes it per run, so two trials of the same task never read
    /// each other's memory.
    pub conversation: String,
    /// What the seat is working on, when the host knows; a retrieval hint.
    pub focus: Option<String>,
    /// Why memory is being recalled now.
    pub moment: RecallMoment,
    /// Character budget the rendered recall must fit within.
    pub budget_chars: usize,
}

/// What kind of thing a [`RememberEntry`] records.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// Something the seat observed: a command it ran and what came back.
    Observation,
    /// An approach that did not work, so nobody tries it again blind.
    FailedAttempt,
    /// A result the seat reached.
    Outcome,
    /// Anything else worth keeping.
    Note,
}

/// One fact a seat leaves in host memory after an activation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberEntry {
    /// What the entry records.
    pub kind: EntryKind,
    /// The entry text, written for a later reader with no other context.
    pub text: String,
}

/// One write to host memory, made after a seat's activation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RememberRequest {
    /// The seat (agent id) whose activation produced the entries.
    pub seat: String,
    /// The host's memory namespace for this conversation; see
    /// [`RecallRequest::conversation`].
    pub conversation: String,
    /// The last desk row the activation had seen, when there was one.
    pub through: Option<Sequence>,
    /// What to remember, in the order the activation produced it.
    pub entries: Vec<RememberEntry>,
}

/// How far into the desk one seat's persistent session has read.
///
/// `through` is inclusive: the highest desk sequence already delivered to the
/// seat, or `None` before anything has been. It is caller-owned state, like
/// [`SharingState`](crate::runtime::SharingState), and this crate stores none.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DeskWatermark {
    /// Highest sequence already in the seat's session.
    pub through: Option<Sequence>,
}

/// The rows a rejoining seat has not seen, and the watermark after them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DeskDelta {
    /// Rows after the old watermark, in the order they were supplied.
    pub rows: Vec<SessionMessage>,
    /// The watermark to commit once the host has delivered `rows`.
    pub watermark: DeskWatermark,
}

/// A session initialization plus what memory recall contributed to it.
///
/// Recalled notes are carried beside the initialization rather than merged
/// into [`SessionContext::notes`](crate::runtime::SessionContext::notes): they
/// are untrusted data written by other activations, and [`Self::framed`] is
/// the one block, under its own heading, that says so.
#[derive(Debug)]
pub struct RecalledSession {
    /// The briefing, context, and history, exactly as
    /// [`initialize_session_with_context`](crate::runtime::initialize_session_with_context)
    /// returned them.
    pub initialization: SessionInitialization,
    /// Notes the memory store returned, in its order; empty on failure.
    pub recalled: Vec<BriefingNote>,
    /// [`frame_recalled`](super::frame_recalled) applied to `recalled` within
    /// the request's budget, or `None` when there is nothing to show.
    pub framed: Option<String>,
    /// The recall failure that was degraded to "no memory", if one was.
    pub failure: Option<Error>,
}
