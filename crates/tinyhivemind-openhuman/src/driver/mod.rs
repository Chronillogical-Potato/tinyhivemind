//! Resumable, host-committed completion episodes.

mod order;
#[cfg(test)]
mod test;

use std::collections::{BTreeMap, BTreeSet};

use openhuman_embed::Agent;
use serde::{Deserialize, Serialize};
use tinyhivemind::{Sequence, speech::Utterance};
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, MessageRoute, Router, RoutingPlan, RoutingPolicy,
    RoutingRequest, RoutingSource, route_broadcast,
};
use tinyhivemind_hive::{
    CompletionEpisodeState, CompletionStep, apply_assignment, apply_completion, completion_status,
};

use crate::{Error, OpenHumanHive, Result};
use order::{broadcast_fallback, extend_pending_order, pending_ids_in_order, prune_pending_order};

/// Caller-owned resumable completion state.
///
/// The receipt map contains only committed event identity, never host session
/// ids. It recognizes exact replay without re-emitting actions and detects a
/// different event reusing the same host sequence. The persisted freshness
/// floor is the maximum episode watermark, participant assignment/completion
/// sequence, or committed receipt sequence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DriverState {
    episode: CompletionEpisodeState,
    receipts: BTreeMap<Sequence, Receipt>,
    freshness_floor: Sequence,
    pending_order: Vec<String>,
    revision: u64,
}

impl DriverState {
    /// Borrow the underlying completion episode state.
    #[must_use]
    pub const fn episode(&self) -> &CompletionEpisodeState {
        &self.episode
    }

    /// Consume the driver wrapper and recover the completion state.
    #[must_use]
    pub fn into_episode(self) -> CompletionEpisodeState {
        self.episode
    }

    /// Return the monotonic revision used to bind pending rounds.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
struct Receipt {
    event: CommittedUtterance,
}

/// One utterance after the host has durably assigned its actual sequence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CommittedUtterance {
    /// Canonical hive id of the author.
    pub author_id: String,
    /// Actual global host sequence of the appended row.
    pub sequence: Sequence,
    /// Accepted tool utterance represented by that row.
    pub utterance: Utterance,
}

/// One pending canonical id and the exact bound `OpenHuman` agent.
#[derive(Clone, Copy, Debug)]
pub struct PendingAgent<'a> {
    /// Canonical hive id.
    pub hive_agent_id: &'a str,
    /// Existing `OpenHuman` runtime handle.
    pub agent: &'a Agent,
}

/// One bounded round the host may run concurrently.
#[derive(Clone, Debug)]
pub struct PendingRound<'a> {
    agents: Vec<PendingAgent<'a>>,
    state: &'a DriverState,
    state_revision: u64,
}

impl PendingRound<'_> {
    /// Borrow pending agents in stable completion-participant order.
    #[must_use]
    pub fn agents(&self) -> &[PendingAgent<'_>] {
        &self.agents
    }

    /// Return whether the completion episode has no currently pending work.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

/// An action proposed to the host after a committed event.
///
/// These values are descriptions only. This crate never executes a turn or
/// appends a transcript row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostAction {
    /// Schedule the named canonical agents to run.
    RunAgents {
        /// Agents in accepted routing order.
        agent_ids: Vec<String>,
        /// Exact accepted plan retained for host audit persistence.
        plan: RoutingPlan,
    },
    /// Deliver an already-committed desk-private message.
    DeliverDm {
        /// Private desk route, never a global direct route.
        route: MessageRoute,
        /// Exact authored message.
        message: String,
    },
}

/// Routing inputs supplied only when folding an agent broadcast.
#[derive(Clone, Copy)]
pub struct BroadcastRouting<'a> {
    /// Primary semantic router.
    pub primary: Option<&'a (dyn Router + 'a)>,
    /// Optional reasoning escalation router.
    pub reasoning: Option<&'a (dyn Router + 'a)>,
    /// Frozen acceptance policy.
    pub policy: &'a RoutingPolicy,
    /// Candidate snapshot version.
    pub roster_version: u64,
    /// Relevant attributed thread context selected by the host.
    pub thread_context: &'a [String],
}

impl std::fmt::Debug for BroadcastRouting<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BroadcastRouting")
            .field("has_primary", &self.primary.is_some())
            .field("has_reasoning", &self.reasoning.is_some())
            .field("policy", self.policy)
            .field("roster_version", &self.roster_version)
            .field("thread_context", &self.thread_context)
            .finish()
    }
}

/// Result of folding committed host events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transition {
    /// Next caller-owned resumable state.
    pub state: DriverState,
    /// Host actions proposed by the committed events.
    pub actions: Vec<HostAction>,
}

/// A bounded driver over one validated `OpenHuman` hive.
#[derive(Debug)]
pub struct CompletionDriver<'a> {
    hive: &'a OpenHumanHive,
    round_width: usize,
}

impl<'a> CompletionDriver<'a> {
    /// Bind a completion driver to one hive and nonzero round width.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ZeroRoundWidth`] for a zero bound.
    pub fn new(hive: &'a OpenHumanHive, round_width: usize) -> Result<Self> {
        if round_width == 0 {
            return Err(Error::ZeroRoundWidth);
        }
        Ok(Self { hive, round_width })
    }

    /// Start and validate new caller-owned completion state.
    ///
    /// # Errors
    ///
    /// Returns an out-of-hive error when the state names another desk or a
    /// participant not present in this hive.
    pub fn start(&self, episode: CompletionEpisodeState) -> Result<DriverState> {
        let freshness_floor = episode_freshness_floor(&episode);
        self.resume(DriverState {
            episode,
            receipts: BTreeMap::new(),
            freshness_floor,
            pending_order: Vec::new(),
            revision: 0,
        })
    }

    /// Resume and validate all caller-owned episode, replay, and freshness state.
    ///
    /// # Errors
    ///
    /// Returns an out-of-hive error when the state names another desk or a
    /// participant or committed author not present in this hive.
    pub fn resume(&self, mut state: DriverState) -> Result<DriverState> {
        if usize::try_from(state.revision) != Ok(state.receipts.len()) {
            return Err(Error::InvalidStateRevision {
                revision: state.revision,
                receipt_count: state.receipts.len(),
            });
        }
        if state.episode.conversation.desk_id != self.hive.desk().id {
            return Err(Error::OutOfHiveEpisode {
                desk_id: self.hive.desk().id.clone(),
            });
        }
        if state.episode.participants.is_empty() {
            return Err(tinyhivemind_hive::Error::NoCompletionParticipants.into());
        }
        let mut participant_ids = BTreeSet::new();
        for participant in &state.episode.participants {
            if participant.agent_id.trim().is_empty() {
                return Err(tinyhivemind_hive::Error::InvalidCompletionParticipant.into());
            }
            if !participant_ids.insert(participant.agent_id.as_str()) {
                return Err(tinyhivemind_hive::Error::DuplicateCompletionParticipant {
                    agent_id: participant.agent_id.clone(),
                }
                .into());
            }
            if self.hive.bound_agent(&participant.agent_id).is_none() {
                return Err(Error::OutOfHiveParticipant {
                    agent_id: participant.agent_id.clone(),
                });
            }
        }
        for (sequence, receipt) in &state.receipts {
            if *sequence != receipt.event.sequence {
                return Err(Error::InvalidReceiptSequence {
                    stored: *sequence,
                    event: receipt.event.sequence,
                });
            }
            if *sequence <= state.episode.watermark {
                return Err(Error::StaleCommittedEvent {
                    sequence: *sequence,
                });
            }
            if self.hive.bound_agent(&receipt.event.author_id).is_none() {
                return Err(Error::OutOfHiveParticipant {
                    agent_id: receipt.event.author_id.clone(),
                });
            }
        }
        for agent_id in &state.pending_order {
            if self.hive.bound_agent(agent_id).is_none() {
                return Err(Error::OutOfHiveParticipant {
                    agent_id: agent_id.clone(),
                });
            }
        }
        prune_pending_order(&mut state);
        let episode_floor = episode_freshness_floor(&state.episode);
        let derived_floor = state
            .receipts
            .last_key_value()
            .map_or(episode_floor, |(sequence, _)| episode_floor.max(*sequence));
        if state.freshness_floor != derived_floor {
            return Err(Error::InvalidFreshnessFloor {
                stored: state.freshness_floor,
                derived: derived_floor,
            });
        }
        Ok(state)
    }

    /// Return the next bounded round without changing state.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownBoundAgent`] if validated state was externally
    /// replaced with an unbound participant.
    pub fn pending_round<'b>(&'b self, state: &'b DriverState) -> Result<PendingRound<'b>> {
        let complete = matches!(
            completion_status(&state.episode),
            CompletionStep::Complete { .. }
        );
        let pending_ids = pending_ids_in_order(state, complete);
        let agents = pending_ids
            .into_iter()
            .take(self.round_width)
            .map(|id| {
                self.hive
                    .bound_agent(id)
                    .map(|agent| PendingAgent {
                        hive_agent_id: id,
                        agent,
                    })
                    .ok_or_else(|| Error::UnknownBoundAgent {
                        agent_id: id.to_owned(),
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(PendingRound {
            agents,
            state,
            state_revision: state.revision,
        })
    }

    /// Fold one host-committed utterance and propose subsequent host actions.
    ///
    /// Exact replay of an already-folded event returns no actions and unchanged
    /// state. State never advances before this committed form is supplied.
    ///
    /// # Errors
    ///
    /// Returns typed stale, duplicate-sequence, membership, routing, and
    /// completion errors.
    pub async fn apply_committed(
        &self,
        state: &DriverState,
        event: CommittedUtterance,
        routing: Option<BroadcastRouting<'_>>,
    ) -> Result<Transition> {
        self.apply_committed_with_fallback(state, event, routing, None)
            .await
    }

    async fn apply_committed_with_fallback(
        &self,
        state: &DriverState,
        event: CommittedUtterance,
        routing: Option<BroadcastRouting<'_>>,
        broadcast_fallback: Option<&str>,
    ) -> Result<Transition> {
        if let Some(replay) = self.replay_or_validate(state, &event)? {
            return Ok(replay);
        }

        let mut episode = state.episode.clone();
        let mut pending_order = state.pending_order.clone();
        let actions = match &event.utterance {
            Utterance::Post { .. } => Vec::new(),
            Utterance::CompleteEpisode { .. } => {
                episode = apply_completion(&episode, &event.author_id, event.sequence)?;
                Vec::new()
            }
            Utterance::Dm { to, message } => vec![HostAction::DeliverDm {
                route: self
                    .hive
                    .resolve_dm(&event.author_id, to, self.round_width)?,
                message: message.clone(),
            }],
            Utterance::Broadcast { message } => {
                let Some(routing) = routing else {
                    return Err(Error::MissingBroadcastRouting);
                };
                let fallback_responder = match broadcast_fallback {
                    Some(fallback) => fallback,
                    None => Self::broadcast_fallback_for(
                        &state.episode,
                        &state.pending_order,
                        &event.author_id,
                    )?,
                };
                let request = self.broadcast_request(&episode, &event.author_id, message, routing);
                let plan = route_broadcast(
                    routing.primary,
                    routing.reasoning,
                    &request,
                    fallback_responder,
                )
                .await;
                let recipients = route_ids(&plan);
                if recipients.len() > self.round_width {
                    return Err(Error::BroadcastTooWide {
                        recipient_count: recipients.len(),
                        round_width: self.round_width,
                    });
                }
                if recipients.iter().any(|id| id == &event.author_id) {
                    return Err(Error::BroadcastIncludesAuthor {
                        agent_id: event.author_id.clone(),
                    });
                }
                for id in &recipients {
                    if self.hive.bound_agent(id).is_none() {
                        return Err(Error::UnknownBoundAgent {
                            agent_id: id.clone(),
                        });
                    }
                }
                if !recipients.is_empty() {
                    episode = apply_assignment(
                        &episode,
                        recipients.iter().map(String::as_str),
                        event.sequence,
                    )?;
                    extend_pending_order(&mut pending_order, &recipients);
                }
                if recipients.is_empty() {
                    Vec::new()
                } else {
                    vec![HostAction::RunAgents {
                        agent_ids: recipients,
                        plan,
                    }]
                }
            }
        };
        let mut next = DriverState {
            episode,
            receipts: state.receipts.clone(),
            freshness_floor: event.sequence,
            pending_order,
            revision: state
                .revision
                .checked_add(1)
                .ok_or(Error::StateRevisionOverflow)?,
        };
        next.receipts.insert(event.sequence, Receipt { event });
        prune_pending_order(&mut next);
        Ok(Transition {
            state: next,
            actions,
        })
    }

    fn replay_or_validate(
        &self,
        state: &DriverState,
        event: &CommittedUtterance,
    ) -> Result<Option<Transition>> {
        if let Some(receipt) = state.receipts.get(&event.sequence) {
            if receipt.event == *event {
                return Ok(Some(Transition {
                    state: state.clone(),
                    actions: Vec::new(),
                }));
            }
            return Err(Error::DuplicateCommittedSequence {
                sequence: event.sequence,
            });
        }
        if event.sequence <= state.freshness_floor {
            return Err(Error::StaleCommittedEvent {
                sequence: event.sequence,
            });
        }
        if self.hive.bound_agent(&event.author_id).is_none() {
            return Err(Error::OutOfHiveParticipant {
                agent_id: event.author_id.clone(),
            });
        }
        Ok(None)
    }

    /// Fold exactly one committed result for every agent in a proposed round.
    /// An exact receipt-only replay returns unchanged without host actions before round, count, or author validation; it is a safe no-op because every event exactly matches its receipt.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PartialRound`] for a wrong count or [`Error::UnexpectedRoundAuthor`] for wrong authors. Exact replay returns successfully before both checks.
    pub async fn apply_committed_round(
        &self,
        state: &DriverState,
        round: &PendingRound<'_>,
        events: Vec<CommittedUtterance>,
        routing: Option<BroadcastRouting<'_>>,
    ) -> Result<Transition> {
        if Self::is_exact_round_replay(state, &events) {
            return Ok(Transition {
                state: state.clone(),
                actions: Vec::new(),
            });
        }
        Self::validate_round_state(state, round)?;
        if events.len() != round.agents.len() {
            return Err(Error::PartialRound {
                expected: round.agents.len(),
                received: events.len(),
            });
        }
        let expected: BTreeSet<_> = round
            .agents
            .iter()
            .map(|pending| pending.hive_agent_id)
            .collect();
        let actual: BTreeSet<_> = events
            .iter()
            .map(|event| event.author_id.as_str())
            .collect();
        if actual.len() != events.len() || actual != expected {
            if let Some(event) = events
                .iter()
                .find(|event| !expected.contains(event.author_id.as_str()))
            {
                return Err(Error::UnexpectedRoundAuthor {
                    agent_id: event.author_id.clone(),
                });
            }
            return Err(Error::PartialRound {
                expected: expected.len(),
                received: actual.len(),
            });
        }
        let mut events = events;
        events.sort_by_key(|event| event.sequence);
        let broadcast_fallbacks = self.preflight_round_events(state, &events, routing)?;
        let mut next = state.clone();
        let mut actions = Vec::new();
        for event in events {
            let fallback = broadcast_fallbacks.get(&event.sequence).map(String::as_str);
            let transition = self
                .apply_committed_with_fallback(&next, event, routing, fallback)
                .await?;
            next = transition.state;
            actions.extend(transition.actions);
        }
        Ok(Transition {
            state: next,
            actions,
        })
    }

    fn is_exact_round_replay(state: &DriverState, events: &[CommittedUtterance]) -> bool {
        if events.is_empty() {
            return false;
        }
        let mut sequences = BTreeSet::new();
        events.iter().all(|event| {
            sequences.insert(event.sequence)
                && state
                    .receipts
                    .get(&event.sequence)
                    .is_some_and(|receipt| receipt.event == *event)
        })
    }

    fn validate_round_state(state: &DriverState, round: &PendingRound<'_>) -> Result<()> {
        if round.state_revision < state.revision {
            return Err(Error::StaleRound {
                round_revision: round.state_revision,
                state_revision: state.revision,
            });
        }
        if round.state_revision != state.revision || round.state != state {
            return Err(Error::MismatchedRound {
                round_revision: round.state_revision,
                state_revision: state.revision,
            });
        }
        Ok(())
    }

    fn preflight_round_events(
        &self,
        state: &DriverState,
        events: &[CommittedUtterance],
        routing: Option<BroadcastRouting<'_>>,
    ) -> Result<BTreeMap<Sequence, String>> {
        let mut newest = state.freshness_floor;
        let mut new_sequences = BTreeSet::new();
        let mut episode = state.episode.clone();
        let mut broadcast_fallbacks = BTreeMap::new();

        for event in events {
            if let Some(receipt) = state.receipts.get(&event.sequence) {
                if receipt.event != *event {
                    return Err(Error::DuplicateCommittedSequence {
                        sequence: event.sequence,
                    });
                }
                continue;
            }
            if !new_sequences.insert(event.sequence) {
                return Err(Error::DuplicateCommittedSequence {
                    sequence: event.sequence,
                });
            }
            if event.sequence <= newest {
                return Err(Error::StaleCommittedEvent {
                    sequence: event.sequence,
                });
            }
            newest = event.sequence;

            match &event.utterance {
                Utterance::Post { .. } => {}
                Utterance::CompleteEpisode { .. } => {
                    episode = apply_completion(&episode, &event.author_id, event.sequence)?;
                }
                Utterance::Dm { to, .. } => {
                    self.hive
                        .resolve_dm(&event.author_id, to, self.round_width)?;
                }
                Utterance::Broadcast { .. } => {
                    if routing.is_none() {
                        return Err(Error::MissingBroadcastRouting);
                    }
                    let fallback = Self::broadcast_fallback_for(
                        &episode,
                        &state.pending_order,
                        &event.author_id,
                    )?;
                    broadcast_fallbacks.insert(event.sequence, fallback.to_owned());
                }
            }
        }
        Ok(broadcast_fallbacks)
    }

    fn broadcast_fallback_for<'state>(
        episode: &'state CompletionEpisodeState,
        pending_order: &'state [String],
        author_id: &str,
    ) -> Result<&'state str> {
        broadcast_fallback(episode, pending_order, author_id).ok_or_else(|| {
            Error::NoBroadcastFallback {
                agent_id: author_id.to_owned(),
            }
        })
    }

    fn broadcast_request(
        &self,
        episode: &CompletionEpisodeState,
        author_id: &str,
        message: &str,
        routing: BroadcastRouting<'_>,
    ) -> RoutingRequest {
        let participants: BTreeSet<_> = episode
            .participants
            .iter()
            .map(|participant| participant.agent_id.as_str())
            .collect();
        RoutingRequest {
            message: message.to_owned(),
            source: RoutingSource::AgentBroadcast {
                author_id: author_id.to_owned(),
            },
            conversation: ConversationRef {
                id: self.hive.desk().id.clone(),
                kind: ConversationKind::Desk,
                thread_root: episode.conversation.thread_root,
            },
            desk_purpose: self.hive.desk().description.clone(),
            thread_context: routing.thread_context.to_vec(),
            candidates: self
                .hive
                .graph()
                .candidates
                .iter()
                .filter(|candidate| {
                    candidate.id != author_id && participants.contains(candidate.id.as_str())
                })
                .cloned()
                .collect(),
            roster_version: routing.roster_version,
            policy: RoutingPolicy {
                round_width: routing.policy.round_width.min(self.round_width),
                ..routing.policy.clone()
            },
        }
    }
}

fn episode_freshness_floor(episode: &CompletionEpisodeState) -> Sequence {
    episode
        .participants
        .iter()
        .fold(episode.watermark, |floor, participant| {
            floor
                .max(participant.assigned_at)
                .max(participant.completed_at.unwrap_or(episode.watermark))
        })
}

fn route_ids(plan: &RoutingPlan) -> Vec<String> {
    match plan {
        RoutingPlan::One { responder_id, .. } | RoutingPlan::Fallback { responder_id, .. } => {
            vec![responder_id.clone()]
        }
        RoutingPlan::Hive {
            primary_id,
            invited_ids,
            ..
        } => std::iter::once(primary_id.clone())
            .chain(invited_ids.iter().cloned())
            .collect(),
        RoutingPlan::Clarify { .. } => Vec::new(),
    }
}
