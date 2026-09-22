//! Deterministic caller-owned ordering for pending completion work.

use std::collections::BTreeSet;

use super::DriverState;
use super::ledger::open_assignment;
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

/// Pending seats owed a turn, in accepted order, then settled seats that owe
/// an answer to a question asked of them.
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
    let participants = state
        .episode
        .participants
        .iter()
        .map(|participant| participant.agent_id.as_str());
    for id in state
        .pending_order
        .iter()
        .map(String::as_str)
        .chain(participants.clone())
    {
        if pending.contains(id) && owed_a_turn(state, id) && scheduled.insert(id) {
            ordered.push(id);
        }
    }
    for id in participants {
        if !pending.contains(id) && owes_an_answer(state, id) && scheduled.insert(id) {
            ordered.push(id);
        }
    }
    ordered
}

/// Pending seats that are not owed a turn: they ran for what they hold and
/// have been shown everything committed.
pub(super) fn stalled_ids(state: &DriverState) -> Vec<String> {
    state
        .episode
        .participants
        .iter()
        .filter(|participant| participant.is_pending())
        .map(|participant| participant.agent_id.as_str())
        .filter(|id| !owed_a_turn(state, id))
        .map(str::to_owned)
        .collect()
}

/// Owed a turn on the assignment it holds: it has not run for that assignment
/// at all, or rows have been committed it has not been shown. The first clause
/// is what invokes a seat the first time and a reassigned one whose handoff
/// sits below its watermark; without it a seat can be given work and never
/// asked to do it. A host that never reports delivery keeps every pending
/// seat owed, which is the behaviour before delivery was tracked.
fn owed_a_turn(state: &DriverState, agent_id: &str) -> bool {
    let Some(assigned_at) = open_assignment(&state.episode, agent_id) else {
        return false;
    };
    state.seen.ran_for.get(agent_id) != Some(&assigned_at)
        || state
            .seen
            .delivered_through
            .get(agent_id)
            .is_none_or(|through| *through < state.freshness_floor)
}

/// Asked something after it was last shown anything, so it has not answered.
fn owes_an_answer(state: &DriverState, agent_id: &str) -> bool {
    let shown = state.seen.delivered_through.get(agent_id).copied();
    state
        .ledger
        .asked_of(agent_id)
        .any(|asked_at| shown.is_none_or(|through| through < asked_at))
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
