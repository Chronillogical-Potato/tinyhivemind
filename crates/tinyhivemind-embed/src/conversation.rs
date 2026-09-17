//! Explicit host-neutral conversation surfaces and outbound message routes.

use serde::{Deserialize, Serialize};
use tinyhivemind::Sequence;

/// The semantic surface on which a turn is running.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    /// A desk channel or a thread rooted in one.
    Desk,
    /// A person-to-agent or agent-to-agent direct conversation.
    Direct,
    /// The host's general conversation.
    General,
    /// A workflow, task, or card conversation.
    Workflow,
}

/// A canonical host conversation, independent of agent session identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConversationRef {
    /// Canonical host-owned conversation id.
    pub id: String,
    /// Semantic conversation surface.
    pub kind: ConversationKind,
    /// Root row for a desk thread.
    pub thread_root: Option<Sequence>,
}

impl ConversationRef {
    /// Return whether this conversation may open a hive episode.
    #[must_use]
    pub const fn may_open_hive(&self) -> bool {
        matches!(self.kind, ConversationKind::Desk)
    }
}

/// Where an agent-authored message should be durably written.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageRoute {
    /// Reply on the conversation where this turn is running.
    CurrentConversation,
    /// Write to the host's canonical direct conversation with one agent.
    DirectAgent {
        /// Canonical recipient agent id.
        agent_id: String,
    },
    /// Write a private row inside the current desk.
    DeskAside {
        /// Canonical recipient agent ids.
        recipient_ids: Vec<String>,
    },
    /// Ask another desk and return its answer as non-voting evidence.
    DeskReferral {
        /// Canonical destination desk id.
        desk_id: String,
    },
}
