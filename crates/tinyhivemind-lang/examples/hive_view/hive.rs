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
    /// share of the hive's seats (see [`desk_policy`]).
    pub(crate) fn of(lowered: &LoweredHive, policy: &EpisodePolicy, opening: &str) -> Self {
        let total: usize = lowered.desks.iter().map(|desk| desk.members.len()).sum();
        let rooms = lowered
            .desks
            .iter()
            .map(|desk| {
                Room::of(
                    lowered,
                    desk,
                    desk_policy(policy, desk.members.len(), total),
                    opening.to_owned(),
                )
            })
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

/// A desk's share of the package's episode policy.
///
/// The package declares one policy for the whole hive. A desk of `size` of
/// the hive's `total` seats takes the same share of the quorum threshold and
/// the turn budget, rounded up, so the desks together spend what the package
/// declared; widths are capped at the desk's size. Nothing else changes.
pub(crate) fn desk_policy(policy: &EpisodePolicy, size: usize, total: usize) -> EpisodePolicy {
    let size_u32 = u32::try_from(size.max(1)).unwrap_or(u32::MAX);
    let total = u64::try_from(total.max(1)).unwrap_or(u64::MAX);
    let share = |value: u32| -> u32 {
        let scaled = (u64::from(value) * u64::from(size_u32)).div_ceil(total);
        u32::try_from(scaled).unwrap_or(u32::MAX)
    };
    let mut desk = *policy;
    desk.quorum.threshold = share(policy.quorum.threshold).clamp(1, size_u32);
    desk.turn_budget = share(policy.turn_budget).max(1);
    desk.round_width = policy.round_width.clamp(1, size_u32);
    desk.revealed_width = policy.revealed_width.clamp(1, size_u32);
    desk
}
