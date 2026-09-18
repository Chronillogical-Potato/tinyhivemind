//! Completion-driven episodes over explicit agent tool events.
//!
//! This is an alternative to [`crate::episode`], not another termination rung
//! inside it. The host opens a state with the agents it assigned, records an
//! agent's `complete_episode` call with [`apply_completion`], and records the
//! accepted recipients of a semantically routed broadcast with
//! [`apply_assignment`]. No prose, quorum, or turn count is interpreted.

#[cfg(test)]
mod test;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tinyhivemind::{Conversation, Sequence};

use crate::{Error, Result};

/// One agent's latest assignment and explicit completion observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ParticipantCompletion {
    /// Canonical agent id.
    pub agent_id: String,
    /// Sequence at which this agent most recently received work.
    pub assigned_at: Sequence,
    /// Sequence of its matching `complete_episode` call, when one exists.
    pub completed_at: Option<Sequence>,
}

/// Caller-owned state for one completion-driven episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CompletionEpisodeState {
    /// Desk and optional thread on which the episode runs.
    pub conversation: Conversation,
    /// Exclusive lower bound at which the episode opened.
    pub watermark: Sequence,
    /// Assigned agents in stable opening order.
    pub participants: Vec<ParticipantCompletion>,
}

impl CompletionEpisodeState {
    /// Open an episode with the agents that have received its initial work.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoCompletionParticipants`] when no agent is assigned,
    /// [`Error::InvalidCompletionParticipant`] for a blank id, or
    /// [`Error::DuplicateCompletionParticipant`] for a repeated id.
    pub fn opened<I, S>(
        conversation: Conversation,
        watermark: Sequence,
        participants: I,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut seen = BTreeSet::new();
        let mut state = Vec::new();
        for id in participants {
            let id = id.as_ref().trim();
            if id.is_empty() {
                return Err(Error::InvalidCompletionParticipant);
            }
            if !seen.insert(id.to_owned()) {
                return Err(Error::DuplicateCompletionParticipant {
                    agent_id: id.to_owned(),
                });
            }
            state.push(ParticipantCompletion {
                agent_id: id.to_owned(),
                assigned_at: watermark,
                completed_at: None,
            });
        }
        if state.is_empty() {
            return Err(Error::NoCompletionParticipants);
        }
        Ok(Self {
            conversation,
            watermark,
            participants: state,
        })
    }
}

/// Current externally actionable state of a completion-driven episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum CompletionStep {
    /// At least one assigned agent has not completed its latest assignment.
    Active {
        /// Agents whose latest assignment remains open, in stable episode order.
        pending_ids: Vec<String>,
    },
    /// Every assigned agent explicitly completed its latest assignment.
    Complete {
        /// Completed agents in stable episode order.
        completed_ids: Vec<String>,
    },
}

/// Read the episode's status without changing it.
#[must_use]
pub fn status(state: &CompletionEpisodeState) -> CompletionStep {
    let pending_ids: Vec<_> = state
        .participants
        .iter()
        .filter(|participant| participant.completed_at.is_none())
        .map(|participant| participant.agent_id.clone())
        .collect();
    if pending_ids.is_empty() {
        CompletionStep::Complete {
            completed_ids: state
                .participants
                .iter()
                .map(|participant| participant.agent_id.clone())
                .collect(),
        }
    } else {
        CompletionStep::Active { pending_ids }
    }
}

/// Record one agent's explicit `complete_episode` tool call.
///
/// Replaying the exact already-recorded event is idempotent.
///
/// # Errors
///
/// Returns [`Error::UnknownCompletionParticipant`] when the caller was never
/// assigned, or [`Error::StaleCompletionEvent`] when the event is not later
/// than that agent's current assignment.
pub fn apply_completion(
    state: &CompletionEpisodeState,
    agent_id: &str,
    at: Sequence,
) -> Result<CompletionEpisodeState> {
    let mut next = state.clone();
    let Some(participant) = next
        .participants
        .iter_mut()
        .find(|participant| participant.agent_id == agent_id)
    else {
        return Err(Error::UnknownCompletionParticipant {
            agent_id: agent_id.to_owned(),
        });
    };
    if participant.completed_at == Some(at) {
        return Ok(next);
    }
    if at <= participant.assigned_at {
        return Err(Error::StaleCompletionEvent {
            agent_id: agent_id.to_owned(),
            sequence: at,
        });
    }
    participant.completed_at = Some(at);
    Ok(next)
}

/// Record the recipients of one accepted, semantically routed broadcast.
///
/// Assignment reopens only the recipients chosen by the router. The host must
/// pass the bounded accepted plan, never raw model labels.
///
/// # Errors
///
/// Returns [`Error::InvalidCompletionParticipant`] for an empty recipient set
/// or blank id, [`Error::DuplicateCompletionParticipant`] for a repeated id,
/// [`Error::UnknownCompletionParticipant`] for an agent outside the episode,
/// or [`Error::StaleCompletionEvent`] when the assignment does not advance a
/// recipient's current assignment sequence.
pub fn apply_assignment<I, S>(
    state: &CompletionEpisodeState,
    recipients: I,
    at: Sequence,
) -> Result<CompletionEpisodeState>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let recipients: Vec<String> = recipients
        .into_iter()
        .map(|id| id.as_ref().trim().to_owned())
        .collect();
    if recipients.is_empty() || recipients.iter().any(String::is_empty) {
        return Err(Error::InvalidCompletionParticipant);
    }
    let unique: BTreeSet<_> = recipients.iter().map(String::as_str).collect();
    if unique.len() != recipients.len() {
        let repeated = recipients
            .iter()
            .find(|id| recipients.iter().filter(|held| held == id).count() > 1)
            .map(String::as_str)
            .unwrap_or_default();
        return Err(Error::DuplicateCompletionParticipant {
            agent_id: repeated.to_owned(),
        });
    }
    let mut next = state.clone();
    for id in recipients {
        let Some(participant) = next
            .participants
            .iter_mut()
            .find(|participant| participant.agent_id == id)
        else {
            return Err(Error::UnknownCompletionParticipant { agent_id: id });
        };
        if at <= participant.assigned_at {
            return Err(Error::StaleCompletionEvent {
                agent_id: id,
                sequence: at,
            });
        }
        participant.assigned_at = at;
        participant.completed_at = None;
    }
    Ok(next)
}
