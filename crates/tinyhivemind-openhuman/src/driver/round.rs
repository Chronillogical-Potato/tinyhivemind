//! Folding a whole pending round at once.

use std::collections::{BTreeMap, BTreeSet};

use tinyhivemind::{Sequence, speech::Utterance};
use tinyhivemind_hive::apply_completion;

use super::ledger::open_assignment;
use super::{
    BroadcastRouting, CommittedUtterance, CompletionDriver, DriverState, PendingRound, Transition,
};
use crate::{Error, Result};

impl CompletionDriver<'_> {
    /// Fold exactly one committed result for every agent in a proposed round.
    ///
    /// An exact receipt-only replay returns unchanged without host actions
    /// before round, count, or author validation; it is a safe no-op because
    /// every event exactly matches its receipt.
    ///
    /// # Errors
    ///
    /// Returns [`Error::PartialRound`] for a wrong count or
    /// [`Error::UnexpectedRoundAuthor`] for wrong authors. Exact replay returns
    /// successfully before both checks.
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
        // What the host reported about delivery since the round was proposed
        // does not invalidate it: the round binds to what was committed.
        if round.state_revision != state.revision || !round.state.same_commitments(state) {
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
                    // Benign for a settled seat, as in the per-event fold.
                    if open_assignment(&episode, &event.author_id).is_some() {
                        episode = apply_completion(&episode, &event.author_id, event.sequence)?;
                    }
                }
                Utterance::Dm { to, .. } => {
                    self.hive
                        .resolve_dm(&event.author_id, to, self.round_width)?;
                }
                Utterance::Ask { to, .. } => {
                    self.hive.resolve_dm(
                        &event.author_id,
                        std::slice::from_ref(to),
                        self.round_width,
                    )?;
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
}
