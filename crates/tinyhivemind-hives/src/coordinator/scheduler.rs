//! FIFO reservations, concurrent runner invocation, and cancellation recovery.
use super::{
    AgentRunner, Coordinator, Destination, EpisodeContext, RunReport, TurnDisposition, TurnOutcome,
    TurnRequest, conduct, interrupt,
};
use crate::{DeliveryStatus, Error, Result, RunningTurn};
use futures::{StreamExt, stream::FuturesUnordered};
use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::Ordering},
};

pub(super) struct Claim {
    runner: Arc<dyn AgentRunner>,
    request: TurnRequest,
}
#[derive(Clone)]
enum Work {
    Direct(usize),
    Episode(usize, usize),
}
/// Interrupted reservations are persisted even if the drain future is dropped.
struct Reservations {
    coordinator: Coordinator,
    agents: BTreeSet<String>,
}
impl Drop for Reservations {
    fn drop(&mut self) {
        if self.agents.is_empty() {
            return;
        }
        // A storage failure cannot be reported from Drop. Durable running records
        // remain discoverable by new() even if this best-effort checkpoint fails.
        let _ = self.coordinator.update(|state| {
            for agent in &self.agents {
                interrupt(state, agent, "scheduler cancelled during turn");
            }
            Ok(())
        });
    }
}
impl Coordinator {
    /// Drain eligible work, leaving parked or unattached agents queued.
    /// Dropping the future interrupts started turns without replaying their effects.
    /// # Errors
    /// Returns storage or malformed durable-conductor errors; runner failures are
    /// recorded in interruptions and the report while other agents continue.
    pub async fn run_until_idle(&self) -> Result<RunReport> {
        let _scheduler = self.inner.scheduler.lock().await;
        let mut guard = Reservations {
            coordinator: self.clone(),
            agents: BTreeSet::new(),
        };
        let mut futures = FuturesUnordered::new();
        let mut report = RunReport::default();
        loop {
            let (changed, conductor_failures) = self.advance_report().await?;
            report.failed += conductor_failures;
            if !self.inner.shutdown.load(Ordering::Acquire) {
                let capacity = self.inner.options.round_width.saturating_sub(futures.len());
                for claim in self.claim(capacity)? {
                    let agent_id = claim.request.agent_id.clone();
                    guard.agents.insert(agent_id.clone());
                    futures.push(async move { (agent_id, claim.runner.run(claim.request).await) });
                }
            }
            if futures.is_empty() {
                if changed && !self.inner.shutdown.load(Ordering::Acquire) {
                    continue;
                }
                break;
            }
            let notification = self.inner.notify.notified();
            tokio::pin!(notification);
            tokio::select! {
                outcome = futures.next() => {
                    if let Some((agent_id, outcome)) = outcome {
                        match self.finish(&agent_id, outcome)? {
                            TurnDisposition::Completed => report.completed += 1,
                            TurnDisposition::Parked => report.parked += 1,
                            TurnDisposition::Failed(_) => report.failed += 1,
                        }
                        guard.agents.remove(&agent_id);
                    }
                }
                () = &mut notification => {}
            }
        }
        Ok(report)
    }
    /// Wait for new work until shutdown, draining each eligible queue.
    /// # Errors
    /// Returns persistence or malformed conductor-state errors.
    pub async fn run(&self) -> Result<()> {
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            // Register before draining so an arrival between idle and await is retained.
            notified.as_mut().enable();
            self.run_until_idle().await?;
            if self.inner.shutdown.load(Ordering::Acquire) {
                return Ok(());
            }
            notified.await;
        }
    }
    #[cfg(test)]
    pub(super) async fn advance(&self) -> Result<bool> {
        Ok(self.advance_report().await?.0)
    }
    async fn advance_report(&self) -> Result<(bool, usize)> {
        loop {
            let original = self.lock()?.durable.clone();
            let mut next = original.clone();
            conduct::prepare(&mut next, &self.inner.options).await?;
            if serde_json::to_vec(&original)? == serde_json::to_vec(&next)? {
                return Ok((false, 0));
            }
            let mut live = self.lock()?;
            if live.durable.revision != original.revision {
                continue;
            }
            let failures = next
                .episodes
                .iter()
                .filter(|episode| {
                    episode.failure.is_some()
                        && original
                            .episodes
                            .iter()
                            .find(|old| old.episode_id == episode.episode_id)
                            .is_none_or(|old| old.failure.is_none())
                })
                .count();
            self.commit(&mut live, next)?;
            return Ok((true, failures));
        }
    }
    pub(super) fn claim(&self, capacity: usize) -> Result<Vec<Claim>> {
        if capacity == 0 {
            return Ok(Vec::new());
        }
        let mut live = self.lock()?;
        let mut next = live.durable.clone();
        let candidates = candidates(&next);
        let mut selected = BTreeSet::new();
        let mut claims = Vec::new();
        // Pending positions are removed by seat after selection; saved indices
        // remain stable while this pass captures all proposed work.
        let mut removals = Vec::new();
        for (_, agent_id, work) in candidates {
            if claims.len() >= capacity {
                break;
            }
            if selected.contains(&agent_id)
                || next.running.contains_key(&agent_id)
                || next.agents.get(&agent_id).is_some_and(|agent| agent.parked)
            {
                continue;
            }
            let Some(runner) = live.runners.get(&agent_id).cloned() else {
                continue;
            };
            let memberships = next
                .hives
                .values()
                .filter(|hive| hive.members.contains(&agent_id))
                .cloned()
                .collect();
            let (messages, episode, turn, delivery_sequence) = match work {
                Work::Direct(index) => {
                    let delivery = &next.deliveries[index];
                    let message = next
                        .messages
                        .iter()
                        .find(|msg| msg.sequence == delivery.sequence)
                        .cloned()
                        .ok_or_else(|| {
                            Error::InvalidState("direct delivery missing message".into())
                        })?;
                    let sequence = delivery.sequence;
                    next.deliveries[index].status = DeliveryStatus::Running;
                    (vec![message], None, None, Some(sequence))
                }
                Work::Episode(index, turn_index) => {
                    let turn = next.episodes[index].pending[turn_index].clone();
                    let (messages, brief) =
                        conduct::open(&mut next, index, &turn, &self.inner.options)?;
                    let record = &next.episodes[index];
                    let context = EpisodeContext {
                        episode_id: record.episode_id.clone(),
                        hive_id: record.hive.hive_id.clone(),
                        thread: turn.thread().map(|root| root.0).or(record.thread),
                        brief,
                    };
                    removals.push((index, agent_id.clone()));
                    (messages, Some(context), Some(turn), None)
                }
            };
            let session_id = next
                .agents
                .get(&agent_id)
                .and_then(|agent| agent.session_id.clone());
            let request = TurnRequest {
                agent_id: agent_id.clone(),
                session_id,
                messages,
                memberships,
                episode,
            };
            next.running.insert(
                agent_id.clone(),
                RunningTurn {
                    request: request.clone(),
                    turn,
                    actions: Vec::new(),
                    delivery_sequence,
                },
            );
            selected.insert(agent_id);
            claims.push(Claim { runner, request });
        }
        for (index, agent_id) in removals {
            next.episodes[index]
                .pending
                .retain(|turn| turn.seat != agent_id);
        }
        if !claims.is_empty() {
            self.commit(&mut live, next)?;
        }
        Ok(claims)
    }
    fn finish(&self, agent_id: &str, outcome: Result<TurnOutcome>) -> Result<TurnDisposition> {
        self.update(|state| {
            let outcome = match outcome {
                Ok(outcome) => outcome,
                Err(error) => {
                    interrupt(state, agent_id, &error.to_string());
                    return Ok(TurnDisposition::Failed(error.to_string()));
                }
            };
            if outcome.session_id.trim().is_empty() {
                interrupt(state, agent_id, "runner returned empty session identity");
                return Ok(TurnDisposition::Failed(
                    "runner returned empty session identity".into(),
                ));
            }
            if state
                .running
                .get(agent_id)
                .and_then(|run| run.request.session_id.as_ref())
                .is_some_and(|session| *session != outcome.session_id)
            {
                interrupt(
                    state,
                    agent_id,
                    "runner changed continuing session identity",
                );
                return Ok(TurnDisposition::Failed(
                    "runner changed continuing session identity".into(),
                ));
            }
            if !state.running.contains_key(agent_id) {
                return Err(Error::InvalidState(
                    "runner returned without reservation".into(),
                ));
            }
            let agent = state
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| Error::UnknownAgent(agent_id.into()))?;
            // A completed host turn has already committed its conversation,
            // even when finalization rejects acknowledgements and episode actions.
            agent.session_id = Some(outcome.session_id.clone());
            if let TurnDisposition::Failed(reason) = &outcome.disposition {
                interrupt(state, agent_id, reason);
                return Ok(outcome.disposition);
            }
            let running = state
                .running
                .remove(agent_id)
                .ok_or_else(|| Error::InvalidState("runner returned without reservation".into()))?;
            let agent = state
                .agents
                .get_mut(agent_id)
                .ok_or_else(|| Error::UnknownAgent(agent_id.into()))?;
            agent.parked = outcome.disposition == TurnDisposition::Parked;
            if let Some(sequence) = running.delivery_sequence {
                for delivery in &mut state.deliveries {
                    if delivery.sequence == sequence && delivery.agent_id == agent_id {
                        delivery.status = if outcome.disposition == TurnDisposition::Parked {
                            DeliveryStatus::Pending
                        } else {
                            DeliveryStatus::Delivered
                        };
                    }
                }
                if let Some(body) = &outcome.reply {
                    let sequence = super::messaging::next_sequence(state)?;
                    state.messages.push(super::Message {
                        message_id: format!("hivemind:event:{sequence}"),
                        sequence,
                        sender: agent_id.into(),
                        destination: Destination::Agent(running.request.messages[0].sender.clone()),
                        body: body.clone(),
                        thread: None,
                        episode_id: None,
                        only_for: Vec::new(),
                    });
                }
            } else if let (Some(episode), Some(turn)) = (&running.request.episode, &running.turn) {
                let index = state
                    .episodes
                    .iter()
                    .position(|record| record.episode_id == episode.episode_id)
                    .ok_or_else(|| Error::StaleEpisode(episode.episode_id.clone()))?;
                conduct::record(
                    state,
                    index,
                    turn,
                    running.actions,
                    &outcome,
                    &self.inner.options,
                )?;
            }
            Ok(outcome.disposition)
        })
    }
}

fn candidates(state: &crate::StoredState) -> Vec<(u64, String, Work)> {
    let mut candidates = Vec::new();
    for (index, delivery) in state.deliveries.iter().enumerate() {
        if delivery.status == DeliveryStatus::Pending {
            candidates.push((
                delivery.sequence,
                delivery.agent_id.clone(),
                Work::Direct(index),
            ));
        }
    }
    for (index, episode) in state.episodes.iter().enumerate() {
        if episode.finished {
            continue;
        }
        for (turn_index, turn) in episode.pending.iter().enumerate() {
            candidates.push((
                episode.opened_at,
                turn.seat.clone(),
                Work::Episode(index, turn_index),
            ));
        }
    }
    candidates.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    candidates
}
