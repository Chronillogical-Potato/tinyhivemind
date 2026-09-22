//! Completion-driven episodes over explicit agent tool events.
//!
//! This is an alternative to [`crate::episode`], not another termination rung
//! inside it. The host opens a state with the agents it assigned, records an
//! agent's `complete_episode` call with [`apply_completion`], and records the
//! accepted recipients of a semantically routed broadcast with
//! [`apply_assignment`]. No prose, quorum, or turn count is interpreted.
//!
//! A participant holds an **append-only history** of assignments and at most
//! one of them is open. Appending rather than overwriting is what keeps the
//! evidence that an earlier assignment completed, which is what makes
//! [`CompletionEpisodeState::settled`] a quantity that only ever rises; the
//! refusal is what makes "at most one open" a checked precondition rather than
//! host discipline. See
//! `docs/adr/0021-an-assignment-is-appended-rather-than-overwritten.md`.

#[cfg(test)]
mod test;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tinyhivemind::{Conversation, Sequence};

use crate::{Error, Result};

/// One assignment a participant received, and whether it finished.
///
/// `assigned_at` is the record's identity: the sequences in one participant's
/// history increase strictly, so no two records of one participant share one.
/// Nothing else names an assignment, and no agent has to.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AssignmentRecord {
    /// Sequence at which this assignment was given.
    pub assigned_at: Sequence,
    /// Sequence of the `complete_episode` call that finished it.
    pub completed_at: Option<Sequence>,
}

impl AssignmentRecord {
    /// Whether this assignment is still owed.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.completed_at.is_none()
    }
}

/// One agent's assignment history within an episode.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ParticipantCompletion {
    /// Canonical agent id.
    pub agent_id: String,
    /// Every assignment this agent received, oldest first.
    ///
    /// Append-only and strictly increasing in `assigned_at`. At most the last
    /// record is open.
    pub assignments: Vec<AssignmentRecord>,
}

impl ParticipantCompletion {
    /// The assignment this agent still owes, if any.
    ///
    /// Only the last record can be open, so an empty history is settled rather
    /// than pending: a participant that was never assigned anything owes
    /// nothing. [`CompletionEpisodeState::opened`] never produces one, and the
    /// total reading keeps `status` a fold rather than a partial function.
    #[must_use]
    pub fn open(&self) -> Option<&AssignmentRecord> {
        self.assignments.last().filter(|record| record.is_open())
    }

    /// Whether this agent has work outstanding.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.open().is_some()
    }

    /// Assignments this agent has finished. Never decreases.
    #[must_use]
    pub fn settled(&self) -> usize {
        self.assignments
            .iter()
            .filter(|record| record.completed_at.is_some())
            .count()
    }
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
                assignments: vec![AssignmentRecord {
                    assigned_at: watermark,
                    completed_at: None,
                }],
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

    /// Assignments finished across the whole episode.
    ///
    /// The one quantity here that only ever rises: records are appended and
    /// never removed, and a completion is never cleared, so a reopened
    /// participant adds a record rather than erasing the one it finished. A
    /// termination argument can be made against this; none can be made against
    /// [`status`], which moves both ways.
    #[must_use]
    pub fn settled(&self) -> usize {
        self.participants
            .iter()
            .map(ParticipantCompletion::settled)
            .sum()
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
///
/// `Complete` is necessary for an episode to be over and is not sufficient: a
/// host that queues handoffs, or that has a routing call in flight, holds state
/// this fold cannot see. See the quiescence predicate in
/// `docs/notes/completion-episode-review.md`.
#[must_use]
pub fn status(state: &CompletionEpisodeState) -> CompletionStep {
    let pending_ids: Vec<_> = state
        .participants
        .iter()
        .filter(|participant| participant.is_pending())
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
/// The subject is resolved rather than named: a participant holds at most one
/// open assignment, so there is nothing for an agent to disambiguate and the
/// tool carries no assignment argument.
///
/// Replaying the exact already-recorded event is idempotent.
///
/// # Errors
///
/// Returns [`Error::UnknownCompletionParticipant`] when the caller was never
/// assigned, [`Error::NoOpenAssignment`] when it has nothing outstanding, or
/// [`Error::StaleCompletionEvent`] when the event is not later than the open
/// assignment it would complete.
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
    let Some(record) = participant.assignments.last_mut() else {
        return Err(Error::NoOpenAssignment {
            agent_id: agent_id.to_owned(),
        });
    };
    // Redelivery of the very event already recorded, which a durable medium may
    // do at any time. Checked before the open test, because the record it names
    // is closed precisely because this event closed it.
    if record.completed_at == Some(at) {
        return Ok(next);
    }
    if record.completed_at.is_some() {
        return Err(Error::NoOpenAssignment {
            agent_id: agent_id.to_owned(),
        });
    }
    if at <= record.assigned_at {
        return Err(Error::StaleCompletionEvent {
            agent_id: agent_id.to_owned(),
            sequence: at,
        });
    }
    record.completed_at = Some(at);
    Ok(next)
}

/// Record the recipients of one accepted, semantically routed broadcast.
///
/// Assignment **appends** a record to each recipient rather than replacing the
/// one it holds, so a reopened participant keeps the evidence that its earlier
/// assignment completed. A recipient that still has work open is refused: at
/// most one assignment per participant is open at a time, and a host that
/// queues a handoff for a busy recipient never reaches this path.
///
/// The host must pass the bounded accepted plan, never raw model labels, and
/// never a `Clarify` or `Fallback` outcome — neither of those selected anybody.
///
/// # Errors
///
/// Returns [`Error::InvalidCompletionParticipant`] for an empty recipient set
/// or blank id, [`Error::DuplicateCompletionParticipant`] for a repeated id,
/// [`Error::UnknownCompletionParticipant`] for an agent outside the episode,
/// [`Error::AssignmentWhileOpen`] for a recipient that has not finished what it
/// already holds, or [`Error::StaleCompletionEvent`] when the assignment does
/// not advance past that recipient's most recent one.
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
        if let Some(open) = participant.assignments.last().filter(|r| r.is_open()) {
            return Err(Error::AssignmentWhileOpen {
                agent_id: id,
                assigned_at: open.assigned_at,
            });
        }
        if let Some(latest) = participant.assignments.last()
            && at <= latest.assigned_at
        {
            return Err(Error::StaleCompletionEvent {
                agent_id: id,
                sequence: at,
            });
        }
        participant.assignments.push(AssignmentRecord {
            assigned_at: at,
            completed_at: None,
        });
    }
    Ok(next)
}
