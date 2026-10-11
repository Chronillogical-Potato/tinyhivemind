//! A package of several desks, described for `desks.rs` to run: one room per
//! desk under its share of the package's policy, the roster and desk set
//! core's `referral` reads, and the package's declared links.

use tinyhivemind_core::hive::{EpisodePolicy, desk::Desk, roster::RosterMember};
use tinyhivemind_core::referral::ReferralPolicy;
use tinyhivemind_lang::LoweredHive;

use crate::episode::Room;

/// A package of several desks, ready to run one floor per desk.
#[derive(Clone, Debug)]
pub(crate) struct Hive {
    /// Package id, for reports.
    pub(crate) package: String,
    /// One room per declared desk, in declared order.
    pub(crate) rooms: Vec<Room>,
    /// Every seat of every desk, for the roster `referral` reads.
    pub(crate) everyone: Vec<RosterMember>,
    /// The declared desks, for the desk set `referral` reads.
    pub(crate) desks: Vec<Desk>,
    /// Declared directed links, topology first, without duplicates.
    pub(crate) links: Vec<(String, String)>,
    /// The package's referral policy.
    pub(crate) referral: ReferralPolicy,
}

impl Hive {
    /// One floor per desk of `lowered`, each under `policy` scaled to its
    /// share of the hive's seats (see [`desk_policies`]).
    pub(crate) fn of(lowered: &LoweredHive, policy: &EpisodePolicy, opening: &str) -> Self {
        let sizes: Vec<usize> = lowered
            .desks
            .iter()
            .map(|desk| desk.members.len())
            .collect();
        let rooms = lowered
            .desks
            .iter()
            .zip(desk_policies(policy, &sizes))
            .map(|(desk, own)| Room::of(lowered, desk, own, opening.to_owned()))
            .collect();
        let mut everyone: Vec<RosterMember> = Vec::new();
        for desk in &lowered.desks {
            for member in &desk.members {
                if !everyone.iter().any(|known| &known.id == member) {
                    everyone.push(RosterMember {
                        id: member.clone(),
                        name: Some(member.clone()),
                    });
                }
            }
        }
        let mut links: Vec<(String, String)> = Vec::new();
        for link in lowered.topology.iter().chain(&lowered.referrals) {
            let pair = (link.from.clone(), link.to.clone());
            if !links.contains(&pair) {
                links.push(pair);
            }
        }
        Self {
            package: lowered.id.clone(),
            rooms,
            everyone,
            desks: lowered.desks.clone(),
            links,
            referral: lowered.policies.referral,
        }
    }

    /// The desk whose ending is the hive's: the last declared one.
    pub(crate) fn answer(&self) -> usize {
        self.rooms.len().saturating_sub(1)
    }

    /// Seats across every desk.
    pub(crate) fn seats(&self) -> usize {
        self.everyone.len()
    }

    pub(crate) fn index(&self, desk: &str) -> Option<usize> {
        self.rooms.iter().position(|room| room.desk.id == desk)
    }

    pub(crate) fn linked(&self, from: &str, to: &str) -> bool {
        self.links.iter().any(|(a, b)| a == from && b == to)
    }

    /// Desks with a forward link (from an earlier-declared desk) into `desk`.
    pub(crate) fn upstream(&self, desk: usize) -> Vec<usize> {
        (0..desk)
            .filter(|from| self.linked(&self.rooms[*from].desk.id, &self.rooms[desk].desk.id))
            .collect()
    }
}

/// Every desk's share of the package's episode policy, in `sizes` order.
///
/// The package declares one policy for the whole hive. A desk of `size` of
/// the hive's seats takes the same share of the quorum threshold, rounded
/// up, and of the turn budget, split by largest remainder so the desks
/// together spend exactly the declared budget and never more (a budget
/// smaller than the desk count leaves some desks with none); widths are
/// capped at the desk's size. Nothing else changes.
pub(crate) fn desk_policies(policy: &EpisodePolicy, sizes: &[usize]) -> Vec<EpisodePolicy> {
    let total = u64::try_from(sizes.iter().sum::<usize>().max(1)).unwrap_or(u64::MAX);
    let weight = |size: usize| u64::try_from(size.max(1)).unwrap_or(u64::MAX);
    let budget = u64::from(policy.turn_budget);
    let mut turns: Vec<u64> = sizes
        .iter()
        .map(|size| budget * weight(*size) / total)
        .collect();
    let mut by_remainder: Vec<usize> = (0..sizes.len()).collect();
    by_remainder.sort_by_key(|at| std::cmp::Reverse(budget * weight(sizes[*at]) % total));
    let leftover = budget.saturating_sub(turns.iter().sum());
    for at in by_remainder
        .into_iter()
        .take(usize::try_from(leftover).unwrap_or(0))
    {
        turns[at] += 1;
    }
    sizes
        .iter()
        .zip(turns)
        .map(|(size, turn_budget)| {
            let size_u32 = u32::try_from((*size).max(1)).unwrap_or(u32::MAX);
            let threshold = (u64::from(policy.quorum.threshold) * weight(*size)).div_ceil(total);
            let mut desk = *policy;
            desk.quorum.threshold = u32::try_from(threshold)
                .unwrap_or(u32::MAX)
                .clamp(1, size_u32);
            desk.turn_budget = u32::try_from(turn_budget).unwrap_or(u32::MAX);
            desk.round_width = policy.round_width.clamp(1, size_u32);
            desk.revealed_width = policy.revealed_width.clamp(1, size_u32);
            desk
        })
        .collect()
}
