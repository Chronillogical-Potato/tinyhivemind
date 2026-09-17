//! Explicit host-neutral conversation surfaces and outbound message routes.

use std::{error::Error as StdError, fmt};

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

/// A message route that would violate a deterministic host bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageRouteError {
    /// A desk aside names more recipients than the opening-round limit.
    DeskAsideTooWide {
        /// Number of recipients named by the authored route.
        recipient_count: usize,
        /// Maximum number of recipients the host permits in one round.
        round_width: usize,
    },
}

impl fmt::Display for MessageRouteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeskAsideTooWide {
                recipient_count,
                round_width,
            } => write!(
                formatter,
                "desk aside names {recipient_count} recipients but the round width is {round_width}"
            ),
        }
    }
}

impl StdError for MessageRouteError {}

impl MessageRoute {
    /// Validate this route against the host's opening-round recipient bound.
    ///
    /// Hosts must call this before durably writing a `DeskAside`. The bound is
    /// supplied at the host boundary because it is a routing-policy decision,
    /// rather than a property of a host-neutral wire payload.
    ///
    /// # Errors
    ///
    /// Returns [`MessageRouteError::DeskAsideTooWide`] when a desk aside would
    /// name more recipients than `round_width` permits.
    pub fn validate_for_round_width(&self, round_width: usize) -> Result<(), MessageRouteError> {
        match self {
            Self::DeskAside { recipient_ids } if recipient_ids.len() > round_width => {
                Err(MessageRouteError::DeskAsideTooWide {
                    recipient_count: recipient_ids.len(),
                    round_width,
                })
            }
            Self::CurrentConversation | Self::DirectAgent { .. } | Self::DeskReferral { .. } => {
                Ok(())
            }
            Self::DeskAside { .. } => Ok(()),
        }
    }
}
