//! Stable host-facing session records.

use serde::{Deserialize, Serialize};
use std::fmt;
use tinyhivemind_core::aside::{Audience, Viewer};

/// A monotonically increasing address in the host-owned session log.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Sequence(pub u64);

impl fmt::Display for Sequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The desk and optional thread viewed by one turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Conversation {
    /// Canonical case-sensitive desk id.
    pub desk_id: String,
    /// Operator-facing desk display name.
    pub desk_name: String,
    /// Root sequence for a thread, or `None` for the desk channel.
    pub thread_root: Option<Sequence>,
}

impl Conversation {
    /// Return whether both values identify the same desk and exact thread.
    ///
    /// All General aliases are equivalent. Named desks match by exact
    /// canonical id; display labels do not merge distinct desks.
    #[must_use]
    pub fn equivalent_to(&self, other: &Self) -> bool {
        use tinyhivemind_core::chat::is_general_chat;

        let self_is_general =
            is_general_chat(Some(&self.desk_id)) || is_general_chat(Some(&self.desk_name));
        let other_is_general =
            is_general_chat(Some(&other.desk_id)) || is_general_chat(Some(&other.desk_name));
        self.thread_root == other.thread_root
            && if self_is_general || other_is_general {
                self_is_general && other_is_general
            } else {
                self.desk_id == other.desk_id
            }
    }
}

impl From<&Conversation> for tinyhivemind_core::dispatch::DispatchConversation {
    fn from(conversation: &Conversation) -> Self {
        use tinyhivemind_core::chat::{GENERAL_DESK, is_general_chat};

        let is_general = is_general_chat(Some(&conversation.desk_id))
            || is_general_chat(Some(&conversation.desk_name));
        Self {
            desk_id: if is_general {
                GENERAL_DESK.into()
            } else {
                conversation.desk_id.clone()
            },
            thread_root: conversation.thread_root.map(|sequence| sequence.0),
        }
    }
}

/// The preserved author of a host log row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionAuthor {
    /// A local operator-authored message.
    Operator,
    /// A human participant.
    Person {
        /// Stable person id.
        id: String,
        /// Display label captured with the row.
        label: String,
    },
    /// An agent participant.
    Agent {
        /// Stable agent id.
        id: String,
        /// Display label captured with the row.
        label: String,
    },
    /// A system or workflow source.
    System {
        /// Host-neutral system category.
        kind: String,
        /// Display label captured with the row.
        label: String,
    },
}

/// One raw row borrowed from the host-owned log.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LogMessage {
    /// Global host sequence.
    pub sequence: Sequence,
    /// Stored desk/chat spelling; `None` is General.
    pub chat_id: Option<String>,
    /// Direct parent sequence, if this is a thread reply.
    pub parent: Option<Sequence>,
    /// Preserved row author.
    pub author: SessionAuthor,
    /// Exact authored content.
    pub content: String,
    /// Who this row is addressed to, when that is narrower than its
    /// conversation.
    ///
    /// The key is required rather than defaulted. An absent audience would
    /// silently mean [`Audience::Desk`] — the permissive value — and a writer
    /// that dropped the field on one path would publish a private message
    /// without anything failing. That is the convention `refutation_cap`
    /// established for a policy-bearing field: a payload from before the field
    /// existed fails to decode rather than quietly acquiring a default.
    pub audience: Audience,
}

/// One newest-first page returned by [`super::SessionLog`].
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionPage {
    /// Rows in strictly descending sequence order.
    pub messages: Vec<LogMessage>,
    /// Exclusive cursor for an older page, no newer than the oldest row.
    pub next_before: Option<Sequence>,
}

/// What a collapsed run of unreadable rows stands in for.
///
/// A row a viewer may not read is never dropped from a projection — see
/// [`SessionMessage::readable`] — and a run of them from one aside collapses
/// into a single stub carrying this. One row rather than twelve is the whole
/// point: the reader is a model with a sliding window, and a run of stubs
/// spends that window to say nothing, in the middle of it, where attention is
/// worst.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Elision {
    /// Last sequence in the collapsed run. The first is the stub's own
    /// [`SessionMessage::sequence`], so the run is the inclusive range.
    pub through: Sequence,
    /// How many rows this stub stands in for.
    pub messages: usize,
    /// Where the aside settled in the desk-visible record, when it has.
    ///
    /// This is the first thing a participant said in the open afterwards, and
    /// it is a row the viewer can read. It is what turns a hole in a reader's
    /// context into a resolvable pointer: a non-member does not need the
    /// content, it needs to know that the outcome is at this sequence. `None`
    /// says the aside has not settled yet, which is an actionable prompt to
    /// ask rather than a gap.
    pub settled_at: Option<Sequence>,
}

/// One chronological, attributed message presented to an agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionMessage {
    /// Original host sequence.
    pub sequence: Sequence,
    /// Original author, never collapsed into the viewer.
    pub author: SessionAuthor,
    /// Original nonblank content, or empty when this row was elided.
    ///
    /// Read it through [`Self::readable`] rather than directly. An elided row
    /// carries no content, and a caller that quotes this field without
    /// checking would publish what the audience withheld.
    pub content: String,
    /// Who the row was addressed to.
    pub audience: Audience,
    /// What this row stands in for, when the viewer may not read it.
    pub elided: Option<Elision>,
}

impl SessionMessage {
    /// The authored text, or `None` when this row was elided for the viewer.
    ///
    /// Every caller that quotes content goes through this. A pin excerpt and
    /// a thread opening are verbatim slices of a message,
    /// and each of them reaches an agent's prompt, so the check has to be in
    /// the type rather than in a reviewer's memory.
    #[must_use]
    pub fn readable(&self) -> Option<&str> {
        match self.elided {
            Some(_) => None,
            None => Some(&self.content),
        }
    }
}

/// Parameters for one bounded transcript projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionQuery {
    /// Desk and optional thread to project.
    pub conversation: Conversation,
    /// Who this projection is being assembled for.
    ///
    /// A projection that cannot name its reader cannot narrow for it, which is
    /// why this rides on the query. [`Viewer::Operator`] and
    /// [`Viewer::Person`] read every row in full.
    pub viewer: Viewer,
    /// Exclusive initial upper bound, often the triggering message sequence.
    pub before: Option<Sequence>,
    /// Maximum number of qualifying messages returned.
    pub window: usize,
}
