//! Episode policy construction for the bench binary.
//!
//! Each arm the comparison runs is a small variation on one tuned policy —
//! wider rounds and the other retained controls — and
//! keeping the variations together here makes the relationship between arms
//! visible in one place rather than scattered through `compare.rs`'s totals.
//! [`turn_budget`] and [`quorum_threshold`] are the two knobs every policy
//! here scales with the size of the desk.

use tinyhivemind_hive::{EpisodePolicy, QuorumPolicy};

/// The crate's own conservative default, with the window widened to cover a
/// whole episode so the two hive arms differ only in the knobs the sweep moved.
pub(crate) fn default_policy() -> EpisodePolicy {
    EpisodePolicy {
        round_width: SEQUENTIAL,
        revealed_width: SEQUENTIAL,
        quorum: QuorumPolicy {
            window: 100,
            ..QuorumPolicy::DEFAULT
        },
        ..EpisodePolicy::DEFAULT
    }
}

/// The width every published arm runs at.
///
/// `EpisodePolicy::DEFAULT` runs wider rounds, because a seat is an async
/// session. Every number recorded before ADR 0014 was measured at width one,
/// and an arm that silently changed width would make a comparison against
/// those numbers meaningless — so the published arms ask for width one and the
/// concurrency arms ask for what they are testing. `--round-width` overrides
/// it, and `docs/experiments/` carries what the wider rounds scored.
pub(crate) const SEQUENTIAL: u32 = 1;

/// The policy `--sweep` picks, scaled to the size of the desk.
///
/// The load-bearing knob is the quorum threshold, and it has two bounds rather
/// than one.
///
/// A threshold *above half the desk* is what suppresses deadlock. Five members
/// can put two grounded supporters behind each of two options, and an episode
/// in which two options both carry is deadlocked by definition: no amount of
/// further support resolves it, because both stay above the line. Requiring a
/// majority makes two *disjoint* supporter sets impossible, and the measured
/// deadlock rate falls to zero.
///
/// It does not make deadlock unreachable in general — one member may back two
/// topics, and a member in both supporter sets is not two members. It is
/// unreachable for the simulated participants here, which each back one
/// option. See `QuorumPolicy::for_room`.
///
/// A threshold *below the whole desk* is what keeps a decision reachable.
/// Cross-inhibition removes a silenced advocate from a topic's supporter set
/// and does not put them back, so at unanimity a single grounded `!object`
/// makes quorum unreachable for the rest of the episode. A live three-member
/// room ran into exactly that and spent its whole budget without deciding.
///
/// Between the two: the smallest majority of the desk, and never the whole of
/// it.
pub(crate) fn tuned_policy(agents: usize) -> EpisodePolicy {
    EpisodePolicy {
        turn_budget: turn_budget(agents),
        round_width: SEQUENTIAL,
        revealed_width: SEQUENTIAL,
        blind_round: true,
        dominance_cap: 40,
        repetition_cap: 2,
        quorum: QuorumPolicy {
            threshold: quorum_threshold(agents),
            window: 100,
            require_grounded: true,
        },
        ..EpisodePolicy::DEFAULT
    }
}

/// The tuned policy widened, so a round authorizes several turns at once.
///
/// This is the arm ADR 0014 has to earn its place against, and it can lose.
/// The prediction it tests is that a wider round buys **depth** — a host with
/// async seats waits once for the whole round — without buying correlated
/// error, because members writing at the same time cannot read each other and
/// so a concurrent round is a blind round. If accuracy falls, the loss is the
/// price of the depth and the table says so.
///
/// A width of `0` returns the tuned policy unchanged, which makes the arm
/// bit-identical to `hive+` — the discipline `set_aside_cap` and
/// `--exchange-cap` already follow.
pub(crate) fn widened_policy(tuned: &EpisodePolicy, width: u32) -> EpisodePolicy {
    if width == 0 {
        return *tuned;
    }
    EpisodePolicy {
        round_width: width,
        revealed_width: width,
        ..*tuned
    }
}

/// The tuned policy widened **only while the room is blind**.
///
/// The free half of concurrency, on its own. A blind member cannot read a
/// peer's row whether or not it runs concurrently with that peer, so this arm
/// should score exactly what `hive+` scores while waiting fewer times. It is
/// the control that separates the depth a round buys from the information a
/// wide *revealed* round spends.
pub(crate) fn blind_wide_policy(tuned: &EpisodePolicy, width: u32) -> EpisodePolicy {
    if width == 0 {
        return *tuned;
    }
    EpisodePolicy {
        round_width: width,
        revealed_width: SEQUENTIAL,
        ..*tuned
    }
}

/// Turns a desk needs to reach a majority quorum.
///
/// A blind opening round costs one turn per member before anybody has seen
/// anybody, a majority then has to assemble on one option, and the decision
/// has to be recorded. Three turns per member covers that with room for the
/// objections and questions a real room spends turns on, and it is a cap
/// rather than a cost: a five-member room finishes in under seven turns of the
/// fifteen it is allowed. A budget that does not scale with the desk is what
/// makes a larger room look worse than a smaller one — at a fixed twelve, an
/// eight-member room fails to decide a third of the time and scores 64%; at
/// twenty-four it decides 96% of the time and scores 88%.
pub(crate) fn turn_budget(agents: usize) -> u32 {
    u32::try_from(agents).unwrap_or(5).saturating_mul(3).max(6)
}

/// The smallest majority of a desk that still leaves one member to spare.
pub(crate) fn quorum_threshold(agents: usize) -> u32 {
    let agents = u32::try_from(agents).unwrap_or(u32::MAX);
    let majority = agents / 2 + 1;
    majority.min(agents.saturating_sub(1)).max(2)
}
