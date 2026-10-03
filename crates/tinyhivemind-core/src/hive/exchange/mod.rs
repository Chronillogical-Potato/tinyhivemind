//! Private exchange that does not take the floor.
//!
//! An aside that rides alongside a turn is free but rationed: only the member
//! the attention market authorized may write one, so a room converging in
//! eleven turns writes at most eleven private rows. This module lifts that
//! ration. Between turns, a host may run an **exchange round** in which each
//! eligible member appends at most one private row, taking no floor and
//! producing no turn.
//!
//! The safety argument is one property of the algebra rather than of this
//! module: a private row is dropped by the episode's own fold before it can
//! reach a trace, a standing, the sequence they fold at, the directory or the
//! floor, and `EpisodeState::spent` counts turns rather than rows.
//! [`crate::hive::step`] therefore returns the same step — including the state it
//! commits — for the same desk rows at the same sequences, which
//! `tests/fuzz_invariants.rs` asserts over arbitrary transcripts.
//!
//! The qualifier matters: a private row consumes a sequence, and both
//! `QuorumPolicy::window` and salience decay read a raw sequence distance, so
//! a host allocating sequences live can shift a later desk row out of a window
//! it was inside. The fold ignores private rows; the numbering does not. See
//! the README in this directory for what bounds that in practice.
//!
//! What a round spends is model calls, not turns. That is a real cost and a
//! host's to authorize, so [`ExchangePolicy`] is off by default and carries
//! two finite ceilings a host can read its worst case off before starting.
//!
//! See [`README.md`][readme] in this directory for the design and the host's
//! obligations, [`docs/specs/off-floor-exchange.md`][spec] for the behavior, and
//! [ADR 0012][adr] for the decision.
//!
//! [readme]: https://github.com/tinyhumansai/tinyhivemind/blob/main/crates/tinyhivemind-core/src/hive/exchange/README.md
//!
//! [spec]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/specs/off-floor-exchange.md
//! [adr]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/adr/0012-an-exchange-round-spends-model-calls-not-turns.md

#[cfg(test)]
mod test;

mod types;

pub use types::{ExchangePolicy, ExchangeRound, ExchangeState, NoExchangeReason};

use crate::runtime::{SessionAuthor, SessionMessage, desk::DeskSet, roster::Roster};

use crate::hive::{EpisodeState, Result};

/// Whether members may exchange privately now, and which of them.
///
/// A fold over what the caller already holds: the policy, the episode's state,
/// the transcript, and the two snapshots. Nothing is stored between calls —
/// how much a member has spent is *read back* out of the private rows above
/// the watermark that it authored, so there is no counter that could disagree
/// with the log.
///
/// The round names members; it does not name pairs. Who a member wants to
/// contact is its own judgement, and the audience it writes is validated by
/// [`crate::runtime::aside::aside`] exactly as an aside riding alongside a turn is.
///
/// # Errors
///
/// Returns [`crate::hive::Error::Core`] when the roster or desk snapshot is
/// structurally invalid, or when the episode's desk is unknown.
pub fn exchange(
    policy: &ExchangePolicy,
    state: &EpisodeState,
    opened: ExchangeState,
    transcript: &[SessionMessage],
    roster: &Roster<'_>,
    desks: &DeskSet<'_>,
) -> Result<ExchangeRound> {
    roster.validate()?;
    desks.validate()?;
    let desk_id = desks.resolve_id(&state.conversation.desk_id)?;
    let members: Vec<&str> = desks
        .members(desk_id)?
        .into_iter()
        .filter(|id| roster.active_member(id).is_some())
        .collect();

    let closed = |reason| Ok(ExchangeRound::Closed { reason });
    if !policy.enabled || policy.contact_cap == 0 || policy.round_cap == 0 {
        return closed(NoExchangeReason::Disabled);
    }
    // A pair is the smallest thing that can exchange. One member alone has
    // nobody to contact, and a round that named it would authorize a row that
    // no audience could admit.
    if members.len() < 2 {
        return closed(NoExchangeReason::TooFewMembers);
    }

    let spent = spent_by(transcript, state, &members);
    // Rounds come from the count the host carries, never from the rows in the
    // log. A round in which every named member declined to write leaves no row
    // behind, so the transcript cannot tell it from a round that never
    // happened — and those are the rounds that cost the most per row, because
    // the host paid to ask each member and got nothing back. Inferring the
    // count from authored rows would let `RoundsSpent` arrive arbitrarily late
    // and leave the model-call budget unbounded, which is the one thing the
    // round cap exists to prevent.
    if opened.rounds >= policy.round_cap {
        return closed(NoExchangeReason::RoundsSpent);
    }

    let eligible: Vec<String> = members
        .iter()
        .filter(|id| contacts_of(&spent, id) < policy.contact_cap)
        .map(|id| (*id).to_owned())
        .collect();
    if eligible.is_empty() {
        return closed(NoExchangeReason::ContactsSpent);
    }

    // Clamped per member, then summed — not summed and then clamped. A round
    // gives each member at most one row, so no member can write more than the
    // rounds that remain however much of its contact cap is left, and none can
    // write more than its cap however many rounds remain. Taking the aggregate
    // minimum instead overreports whenever spend is uneven: one member that has
    // spent its cap and one that has spent nothing report the second member's
    // whole cap, when only `rounds_left` of it is reachable.
    let rounds_left = policy.round_cap.saturating_sub(opened.rounds);
    let remaining: u64 = members
        .iter()
        .map(|id| {
            u64::from(
                policy
                    .contact_cap
                    .saturating_sub(contacts_of(&spent, id))
                    .min(rounds_left),
            )
        })
        .sum();

    Ok(ExchangeRound::Open {
        members: eligible,
        remaining,
        next: opened.advanced(),
    })
}

/// Private rows above the watermark, counted by the member that authored them.
///
/// Only rows from current members of this desk count. A row from a retired
/// agent, or from one that belongs to another desk, is not this episode's
/// spend — the same test `live_traces` applies before folding a vote.
fn spent_by<'a>(
    transcript: &[SessionMessage],
    state: &EpisodeState,
    members: &[&'a str],
) -> Vec<(&'a str, u32)> {
    let mut spent: Vec<(&str, u32)> = members.iter().map(|id| (*id, 0)).collect();
    for message in transcript {
        if message.sequence <= state.watermark || message.audience.is_desk() {
            continue;
        }
        let SessionAuthor::Agent { id, .. } = &message.author else {
            continue;
        };
        if let Some(entry) = spent.iter_mut().find(|(member, _)| member == id) {
            entry.1 = entry.1.saturating_add(1);
        }
    }
    spent
}

/// One member's spend, or zero for a member the fold did not see.
fn contacts_of(spent: &[(&str, u32)], id: &str) -> u32 {
    spent
        .iter()
        .find(|(member, _)| *member == id)
        .map_or(0, |(_, count)| *count)
}
