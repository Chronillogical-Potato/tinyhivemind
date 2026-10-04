//! Folding one committed broadcast: route it, then place it.
//!
//! A broadcast always resolves to an owner. Each routed recipient is either
//! assigned now, or -- because it still holds an open assignment and a
//! participant holds at most one (ADR 0021) -- handed a queued [`Handoff`] it
//! receives the moment it completes. A recipient whose queue is full is
//! skipped; if nobody could take the work at all, it stays with its author,
//! who is owed another turn to decide what to do with it. Nothing is dropped:
//! a refused seat is not run, and the author's assignment stays open.

use std::collections::BTreeSet;

use crate::embed::{
    ConversationKind, ConversationRef, RoutingPlan, RoutingPolicy, RoutingRequest, RoutingSource,
    route_broadcast,
};
use crate::hive::{CompletionEpisodeState, apply_assignment, apply_completion};

use super::ledger::{Handoff, latest_assignment, open_assignment};
use super::order::{broadcast_fallback, extend_pending_order};
use super::{
    BroadcastRouting, CommittedUtterance, CompletionDriver, DriverState, HostAction, is_pending,
};
use crate::driver::graph::BoundAgent;
use crate::driver::{Error, Result};

impl<A: BoundAgent> CompletionDriver<'_, A> {
    pub(super) async fn fold_broadcast(
        &self,
        state: &DriverState,
        next: &mut DriverState,
        event: &CommittedUtterance,
        message: &str,
        routing: Option<BroadcastRouting<'_>>,
        broadcast_fallback: Option<&str>,
    ) -> Result<Vec<HostAction>> {
        let Some(routing) = routing else {
            return Err(Error::MissingBroadcastRouting);
        };
        let author = event.author_id.as_str();
        // Admitted before the model is called, so a refusal costs nothing.
        // Charged to the most recent assignment, open or just closed by the
        // seat's own handoff, so completing does not refill the budget.
        let charged_at = latest_assignment(&next.episode, author);
        if let (Some(cap), Some(assigned_at)) = (self.broadcast_budget, charged_at)
            && next.ledger.charged(author, assigned_at) >= cap
        {
            return Err(Error::BudgetSpent {
                agent_id: author.to_owned(),
                assigned_at,
            });
        }
        let fallback_responder = match broadcast_fallback {
            Some(fallback) => fallback,
            None => Self::broadcast_fallback_for(&state.episode, &state.pending_order, author)?,
        };
        let request = self.broadcast_request(&next.episode, author, message, routing);
        let plan = route_broadcast(
            routing.primary,
            routing.reasoning,
            &request,
            fallback_responder,
        )
        .await;
        if let Some(assigned_at) = charged_at {
            next.ledger.charge(author, assigned_at);
        }
        let recipients = route_ids(&plan);
        if recipients.len() > self.round_width {
            return Err(Error::BroadcastTooWide {
                recipient_count: recipients.len(),
                round_width: self.round_width,
            });
        }
        if recipients.iter().any(|id| id == author) {
            return Err(Error::BroadcastIncludesAuthor {
                agent_id: author.to_owned(),
            });
        }
        for id in &recipients {
            if self.hive.bound_agent(id).is_none() {
                return Err(Error::UnknownBoundAgent {
                    agent_id: id.clone(),
                });
            }
        }
        if recipients.is_empty() {
            // Nobody on the desk fits. The work stays with its author, who is
            // owed another turn to decide what to do with it rather than left
            // believing it was handed off.
            next.seen.ran_for.remove(author);
            return Ok(Vec::new());
        }
        let refused = self.place(next, event, message, &recipients)?;
        let placed: Vec<String> = recipients
            .into_iter()
            .filter(|id| !refused.contains(id))
            .collect();
        if placed.is_empty() {
            // Every recipient was refused for capacity. `place` already owes
            // the author another turn; its assignment stays open so the work
            // is not closed on a handoff nobody received.
            return Ok(Vec::new());
        }
        let mut actions = vec![HostAction::RunAgents {
            agent_ids: placed,
            plan,
        }];
        // Handing work off is a finding. Unless the author is still waiting
        // on a question it asked, its part is complete (ADR 0024) -- and a
        // handoff queued for it is handed over now, as at any completion.
        // A seat not yet shown its assignment cannot have finished it: that
        // assignment stays open, and the seat, which has not run for it, is
        // owed a turn. The same guard an explicit completion meets.
        if let Some(assigned_at) = open_assignment(&next.episode, author)
            && next.ledger.awaiting(author).is_none()
            && next
                .seen
                .delivered_through
                .get(author)
                .is_none_or(|through| *through >= assigned_at)
        {
            next.episode = apply_completion(&next.episode, author, event.sequence)?;
            if let Some(handoff) = next.ledger.pop(author) {
                next.episode = apply_assignment(&next.episode, [author], event.sequence)?;
                actions.push(HostAction::DeliverHandoff {
                    agent_id: author.to_owned(),
                    handoff,
                });
            }
        }
        Ok(actions)
    }

    /// Place the work; returns the recipients refused for a full queue.
    fn place(
        &self,
        next: &mut DriverState,
        event: &CommittedUtterance,
        message: &str,
        recipients: &[String],
    ) -> Result<Vec<String>> {
        let author = event.author_id.as_str();
        let mut idle: Vec<&str> = Vec::new();
        let mut queued = 0_usize;
        let mut refused: Vec<String> = Vec::new();
        for id in recipients {
            if !is_pending(&next.episode, id) {
                idle.push(id);
                continue;
            }
            // Full is a fact about this recipient, not about the episode: it
            // filled up while the model was thinking. Skip it and let the
            // outcome below decide who owns the work.
            if next.ledger.queue_len(id) >= self.queue_depth {
                refused.push(id.clone());
                continue;
            }
            next.ledger.push(
                id,
                Handoff {
                    from: author.to_owned(),
                    body: message.to_owned(),
                    origin: event.sequence,
                },
            );
            queued += 1;
        }
        if !idle.is_empty() {
            next.episode = apply_assignment(&next.episode, &idle, event.sequence)?;
        }
        extend_pending_order(&mut next.pending_order, recipients);
        // Nobody could take it and somebody was refused for capacity: the work
        // stays with the author, who must be asked again rather than left to
        // believe it was handed off.
        if idle.is_empty() && queued == 0 && !refused.is_empty() {
            next.seen.ran_for.remove(author);
        }
        Ok(refused)
    }

    pub(super) fn broadcast_fallback_for<'state>(
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

pub(super) fn route_ids(plan: &RoutingPlan) -> Vec<String> {
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
