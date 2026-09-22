//! Deterministic caller-owned ordering for pending completion work.

use std::collections::BTreeSet;

use super::DriverState;
use tinyhivemind_hive::CompletionEpisodeState;

pub(super) fn broadcast_fallback<'state>(
    episode: &'state CompletionEpisodeState,
    pending_order: &'state [String],
    author_id: &str,
) -> Option<&'state str> {
    let participants: BTreeSet<_> = episode
        .participants
        .iter()
        .map(|participant| participant.agent_id.as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let ordered: Vec<_> = pending_order
        .iter()
        .map(String::as_str)
        .chain(
            episode
                .participants
                .iter()
                .map(|participant| participant.agent_id.as_str()),
        )
        .filter(|id| participants.contains(id) && seen.insert(*id))
        .collect();
    let author_index = ordered.iter().position(|id| *id == author_id)?;
    (1..ordered.len())
        .map(|offset| ordered[(author_index + offset) % ordered.len()])
        .next()
}

pub(super) fn pending_ids_in_order(state: &DriverState, complete: bool) -> Vec<&str> {
    if complete {
        return Vec::new();
    }
    let pending: BTreeSet<_> = state
        .episode
        .participants
        .iter()
        .filter(|participant| participant.is_pending())
        .map(|participant| participant.agent_id.as_str())
        .collect();
    let mut scheduled = BTreeSet::new();
    let mut ordered = Vec::new();
    for id in state.pending_order.iter().map(String::as_str).chain(
        state
            .episode
            .participants
            .iter()
            .map(|participant| participant.agent_id.as_str()),
    ) {
        if pending.contains(id) && scheduled.insert(id) {
            ordered.push(id);
        }
    }
    ordered
}

pub(super) fn extend_pending_order(order: &mut Vec<String>, recipients: &[String]) {
    let mut present: BTreeSet<_> = order.iter().cloned().collect();
    order.extend(
        recipients
            .iter()
            .filter(|id| present.insert((*id).clone()))
            .cloned(),
    );
}

pub(super) fn prune_pending_order(state: &mut DriverState) {
    let pending: BTreeSet<_> = state
        .episode
        .participants
        .iter()
        .filter(|participant| participant.is_pending())
        .map(|participant| participant.agent_id.as_str())
        .collect();
    let mut retained = BTreeSet::new();
    state
        .pending_order
        .retain(|id| pending.contains(id.as_str()) && retained.insert(id.clone()));
}
