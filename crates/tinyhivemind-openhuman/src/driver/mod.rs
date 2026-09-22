//! Resumable, host-committed completion episodes.

mod brief;
mod broadcast;
mod ledger;
mod order;
mod round;
#[cfg(test)]
mod test;

use std::collections::{BTreeMap, BTreeSet};

use openhuman_embed::Agent;

use crate::graph::BoundAgent;
use serde::{Deserialize, Serialize};
use tinyhivemind::{Sequence, speech::Utterance};
use tinyhivemind_embed::{MessageRoute, Router, RoutingPlan, RoutingPolicy};
use tinyhivemind_hive::{
    CompletionEpisodeState, CompletionStep, apply_assignment, apply_completion, completion_status,
};

use crate::{Error, OpenHumanHive, Result};
pub use brief::{Channel, ConversationView, EpisodeBrief, standing_contract};
#[cfg(test)]
use broadcast::route_ids;
pub use ledger::{AssignmentSpend, Handoff, Ledger, Seen};
use ledger::{named_ids, open_assignment};
use order::{pending_ids_in_order, prune_pending_order, stalled_ids};

/// Caller-owned resumable completion state.
///
/// The receipt map contains only committed event identity, never host session
/// ids. It recognizes exact replay without re-emitting actions and detects a
/// different event reusing the same host sequence. The persisted freshness
/// floor is the maximum episode watermark, participant assignment/completion
/// sequence, or committed receipt sequence.
///
/// The [`Ledger`] moves only with committed events and is part of what a
/// pending round binds to. [`Seen`] is what the host has reported about
/// delivery and turns; it moves the wake predicate and nothing else.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DriverState {
    episode: CompletionEpisodeState,
    receipts: BTreeMap<Sequence, Receipt>,
    freshness_floor: Sequence,
    pending_order: Vec<String>,
    revision: u64,
    #[serde(default)]
    ledger: Ledger,
    #[serde(default)]
    seen: Seen,
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

    /// Queued handoffs, budgets, and open questions.
    #[must_use]
    pub const fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// What the host has reported about each seat.
    #[must_use]
    pub const fn seen(&self) -> &Seen {
        &self.seen
    }

    /// Record that a seat has been shown every row through `through`.
    ///
    /// This is how a completion becomes checkable against delivery, and how
    /// a seat stops being owed a turn for rows it has already read. A host
    /// that never reports delivery keeps the prior behaviour: every pending
    /// seat is always owed a turn.
    pub fn delivered(&mut self, agent_id: &str, through: Sequence) {
        self.seen.delivered(agent_id, through);
    }

    /// Record that a seat's turn was started for the assignment it holds.
    ///
    /// A committed row from the seat records this on its own; the explicit
    /// call is for a turn that returns without committing anything, which is
    /// the one way a pending seat can otherwise be woken forever.
    pub fn turn_started(&mut self, agent_id: &str) {
        if let Some(assigned_at) = open_assignment(&self.episode, agent_id) {
            self.seen.ran(agent_id, assigned_at);
        }
    }

    /// Record that a seat's last turn did not count: it is owed another for
    /// the assignment it holds.
    ///
    /// The host's call for a turn that returned without saying anything the
    /// episode could record -- a seat asked a question that replied in prose
    /// and called no tool. Without it the seat has run and been shown
    /// everything, so nothing would wake it again.
    pub fn owe_turn(&mut self, agent_id: &str) {
        self.seen.ran_for.remove(agent_id);
    }

    /// Whether the episode is actually over.
    ///
    /// [`CompletionStep::Complete`] is necessary and not sufficient: between a
    /// completion and its queue drain, or while an answer is still owed, the
    /// status reads complete and the episode is not. Gate termination on this.
    #[must_use]
    pub fn quiescent(&self) -> bool {
        matches!(
            completion_status(&self.episode),
            CompletionStep::Complete { .. }
        ) && self.ledger.is_drained()
    }

    /// Pending seats nothing will ever wake: they ran for what they hold and
    /// have been shown everything. The failure is otherwise silent.
    #[must_use]
    pub fn stalled(&self) -> Vec<String> {
        stalled_ids(self)
    }

    /// Everything except what the host reported about delivery and turns.
    fn same_commitments(&self, other: &Self) -> bool {
        self.episode == other.episode
            && self.receipts == other.receipts
            && self.freshness_floor == other.freshness_floor
            && self.pending_order == other.pending_order
            && self.revision == other.revision
            && self.ledger == other.ledger
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
pub struct PendingAgent<'a, A = Agent> {
    /// Canonical hive id.
    pub hive_agent_id: &'a str,
    /// Existing `OpenHuman` runtime handle.
    pub agent: &'a A,
}

impl<A> Clone for PendingAgent<'_, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> Copy for PendingAgent<'_, A> {}

impl<A: std::fmt::Debug> std::fmt::Debug for PendingAgent<'_, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAgent")
            .field("hive_agent_id", &self.hive_agent_id)
            .field("agent", &self.agent)
            .finish()
    }
}

/// One bounded round the host may run concurrently.
#[derive(Clone, Debug)]
pub struct PendingRound<'a, A = Agent> {
    agents: Vec<PendingAgent<'a, A>>,
    state: &'a DriverState,
    state_revision: u64,
}

impl<A: BoundAgent> PendingRound<'_, A> {
    /// Borrow pending agents in stable completion-participant order.
    #[must_use]
    pub fn agents(&self) -> &[PendingAgent<'_, A>] {
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
    ///
    /// For an [`Utterance::Ask`] this is the signal to **open a child
    /// conversation** between the author and the seat named: a thread of the
    /// desk rooted at the ask row, with the two as its participants, run to
    /// its own quiescence. When it concludes, the host cross-posts its outcome
    /// as a private message from the seat asked to the asker; that row is what
    /// releases the asker's hold and wakes it, with the whole conversation in
    /// its context. See ADR 0023.
    DeliverDm {
        /// Private desk route, never a global direct route.
        route: MessageRoute,
        /// Exact authored message.
        message: String,
    },
    /// Hand a queued broadcast to the seat that just came free, and run it.
    ///
    /// The seat was assigned it at the completion that freed it, so the
    /// assignment sits at a row that exists rather than one the journal has
    /// yet to reach.
    DeliverHandoff {
        /// The seat now holding the work.
        agent_id: String,
        /// The handoff, as it was queued.
        handoff: Handoff,
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
pub struct CompletionDriver<'a, A = Agent> {
    hive: &'a OpenHumanHive<A>,
    round_width: usize,
    queue_depth: usize,
    broadcast_budget: Option<u32>,
}

impl<'a, A: BoundAgent> CompletionDriver<'a, A> {
    /// Bind a completion driver to one hive and nonzero round width.
    ///
    /// Each recipient may hold `round_width` queued handoffs, and an
    /// assignment may broadcast without limit; see
    /// [`with_queue_depth`](Self::with_queue_depth) and
    /// [`with_broadcast_budget`](Self::with_broadcast_budget).
    ///
    /// # Errors
    ///
    /// Returns [`Error::ZeroRoundWidth`] for a zero bound.
    pub fn new(hive: &'a OpenHumanHive<A>, round_width: usize) -> Result<Self> {
        if round_width == 0 {
            return Err(Error::ZeroRoundWidth);
        }
        Ok(Self {
            hive,
            round_width,
            queue_depth: round_width,
            broadcast_budget: None,
        })
    }

    /// How many handoffs one recipient may hold before further ones are
    /// refused and the work stays with its author.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ZeroQueueDepth`] for a zero bound.
    pub fn with_queue_depth(mut self, queue_depth: usize) -> Result<Self> {
        if queue_depth == 0 {
            return Err(Error::ZeroQueueDepth);
        }
        self.queue_depth = queue_depth;
        Ok(self)
    }

    /// How many broadcasts one assignment may route before the author is
    /// refused and must finish what it holds. `None` is unlimited.
    #[must_use]
    pub const fn with_broadcast_budget(mut self, budget: Option<u32>) -> Self {
        self.broadcast_budget = budget;
        self
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
            ledger: Ledger::default(),
            seen: Seen::default(),
        })
    }

    /// Resume and validate all caller-owned episode, replay, and freshness state.
    ///
    /// # Errors
    ///
    /// Returns an out-of-hive error when the state names another desk or a
    /// participant, committed author, or ledger entry not present in this hive.
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
            self.bound(&participant.agent_id)?;
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
            self.bound(&receipt.event.author_id)?;
        }
        for agent_id in state
            .pending_order
            .iter()
            .map(String::as_str)
            .chain(named_ids(&state.ledger))
            .chain(state.seen.delivered_through.keys().map(String::as_str))
        {
            self.bound(agent_id)?;
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

    fn bound(&self, agent_id: &str) -> Result<()> {
        if self.hive.bound_agent(agent_id).is_none() {
            return Err(Error::OutOfHiveParticipant {
                agent_id: agent_id.to_owned(),
            });
        }
        Ok(())
    }

    /// Return the next bounded round without changing state.
    ///
    /// A seat is in the round when it holds an open assignment it has not run
    /// for, or holds rows it has not been shown -- or when it owes an answer
    /// to a question asked of it after it was last shown anything.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownBoundAgent`] if validated state was externally
    /// replaced with an unbound participant.
    pub fn pending_round<'b>(&'b self, state: &'b DriverState) -> Result<PendingRound<'b, A>> {
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
    /// Returns typed stale, duplicate-sequence, membership, routing, budget,
    /// and completion errors.
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
        let mut next = state.clone();
        let author = event.author_id.as_str();
        // A row is proof its author ran for what it held, and it answers
        // whoever was waiting on that author. Both before the row's own
        // effect, which may change what the author holds.
        // -- unless the host says the seat has not yet been shown that
        // assignment: a peer's broadcast can assign a seat mid-turn, and the
        // rows it commits then belong to the turn it was already in.
        if let Some(assigned_at) = open_assignment(&state.episode, author)
            && next
                .seen
                .delivered_through
                .get(author)
                .is_none_or(|through| *through >= assigned_at)
        {
            next.seen.ran(author, assigned_at);
        }
        // The answer an asker awaits is the conversation's conclusion,
        // cross-posted to it as a private message from the seat it asked.
        // Nothing that seat says on the open desk counts, so a child
        // conversation still in progress cannot be mistaken for over.
        if let Utterance::Dm { to, .. } = &event.utterance {
            for asker in to {
                next.ledger.answered(author, asker);
            }
        }

        let actions = match &event.utterance {
            Utterance::Post { .. } => Vec::new(),
            Utterance::CompleteEpisode { .. } => Self::fold_completion(&mut next, &event)?,
            Utterance::Dm { to, message } => self.deliver_privately(author, to, message)?,
            Utterance::Ask { to, message } => {
                let actions = self.deliver_privately(author, std::slice::from_ref(to), message)?;
                next.ledger.open_ask(author, to, event.sequence);
                actions
            }
            Utterance::Broadcast { message } => {
                self.fold_broadcast(
                    state,
                    &mut next,
                    &event,
                    message,
                    routing,
                    broadcast_fallback,
                )
                .await?
            }
        };
        next.freshness_floor = event.sequence;
        next.revision = state
            .revision
            .checked_add(1)
            .ok_or(Error::StateRevisionOverflow)?;
        next.receipts.insert(event.sequence, Receipt { event });
        prune_pending_order(&mut next);
        Ok(Transition {
            state: next,
            actions,
        })
    }

    /// Record a completion, then hand the seat one queued handoff if it has any.
    ///
    /// The order is the whole discipline. Completing first records the work
    /// the seat actually did; assigning first would move `assigned_at` past
    /// this row and stale-reject the very completion being recorded. The new
    /// assignment sits at this completion's sequence -- a row that exists --
    /// rather than at the broadcast's origin, which the seat's previous work
    /// would already satisfy, or at some future row the watermark could never
    /// reach.
    fn fold_completion(
        next: &mut DriverState,
        event: &CommittedUtterance,
    ) -> Result<Vec<HostAction>> {
        let author = event.author_id.as_str();
        // A settled seat saying it is done is already true. A seat woken to
        // answer a question, having answered, will often say so; the row is
        // recorded and nothing moves. Refusing it aborted a live episode.
        if open_assignment(&next.episode, author).is_none() {
            return Ok(Vec::new());
        }
        if let Some(waiting) = next.ledger.awaiting(author) {
            return Err(Error::AwaitingReply {
                agent_id: author.to_owned(),
                waiting_on: waiting.keys().cloned().collect(),
            });
        }
        // Checkable only where the host has said what it delivered; a host
        // that never reports keeps the prior, unchecked behaviour.
        if let (Some(assigned_at), Some(delivered_through)) = (
            open_assignment(&next.episode, author),
            next.seen.delivered_through.get(author).copied(),
        ) && delivered_through < assigned_at
        {
            return Err(Error::UndeliveredAssignment {
                agent_id: author.to_owned(),
                assigned_at,
                delivered_through,
            });
        }
        next.episode = apply_completion(&next.episode, author, event.sequence)?;
        let Some(handoff) = next.ledger.pop(author) else {
            return Ok(Vec::new());
        };
        next.episode = apply_assignment(&next.episode, [author], event.sequence)?;
        Ok(vec![HostAction::DeliverHandoff {
            agent_id: author.to_owned(),
            handoff,
        }])
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
        self.bound(&event.author_id)?;
        Ok(None)
    }

    /// One private delivery — a `dm` to its peers or an `ask` to its one seat
    /// — as the action the host performs. Resolving the route here is what
    /// checks the recipients against the hive before anything is sent.
    fn deliver_privately(
        &self,
        author_id: &str,
        to: &[String],
        message: &str,
    ) -> Result<Vec<HostAction>> {
        Ok(vec![HostAction::DeliverDm {
            route: self.hive.resolve_dm(author_id, to, self.round_width)?,
            message: message.to_owned(),
        }])
    }
}

/// Whether this participant still owes work on the assignment it holds.
fn is_pending(episode: &CompletionEpisodeState, agent_id: &str) -> bool {
    episode
        .participants
        .iter()
        .find(|participant| participant.agent_id == agent_id)
        .is_some_and(tinyhivemind_hive::ParticipantCompletion::is_pending)
}

fn episode_freshness_floor(episode: &CompletionEpisodeState) -> Sequence {
    episode
        .participants
        .iter()
        .fold(episode.watermark, |floor, participant| {
            // Across the whole history rather than one slot: a participant now
            // keeps every assignment it has held, and the floor is still the
            // highest sequence any of that work has touched.
            participant.assignments.iter().fold(floor, |floor, record| {
                floor
                    .max(record.assigned_at)
                    .max(record.completed_at.unwrap_or(episode.watermark))
            })
        })
}
