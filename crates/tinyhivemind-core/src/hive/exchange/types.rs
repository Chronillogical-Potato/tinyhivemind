//! Inputs and outputs of the off-floor exchange fold.

use serde::{Deserialize, Serialize};

/// How much private exchange one episode may run off the floor.
///
/// Two independent ceilings, both finite and both readable before an episode
/// starts, so a host knows its worst case rather than discovering it. The
/// total rows an episode can produce is at most
/// `round_cap × max_active_members`, the max taken over the episode since this
/// fold does not freeze a host's roster. Each member is independently bounded
/// by `contact_cap`, whenever it joined.
///
/// Off by default. An exchange round is *n* model calls, which is a real cost
/// and a host's to authorize — see
/// [ADR 0012][adr].
///
/// [adr]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/adr/0012-an-exchange-round-spends-model-calls-not-turns.md
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ExchangePolicy {
    /// Whether any round may open at all.
    pub enabled: bool,
    /// Private rows one member may author across the whole episode.
    pub contact_cap: u32,
    /// Rounds the episode may open at all.
    pub round_cap: u32,
}

impl ExchangePolicy {
    /// Off, which reproduces an episode taken before this mechanism existed.
    pub const DEFAULT: Self = Self {
        enabled: false,
        contact_cap: 0,
        round_cap: 0,
    };
}

impl Default for ExchangePolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How many exchange rounds this episode has already opened.
///
/// Carried by the host, not folded from the transcript, and that is the whole
/// point: a round in which every named member declines to write leaves no row
/// behind, so nothing in the log distinguishes it from a round that never
/// happened. Inferring the count from authored rows therefore under-counts
/// exactly the rounds that cost the most per row — and a host whose
/// participants decline can pay for asking each of them, without limit, while
/// `round_cap` never closes.
///
/// The host already knows how many times it called [`crate::hive::exchange()`]. It
/// carries this the same way it carries [`crate::hive::EpisodeState`]: opened once,
/// advanced by the value the last round returned, and never stored by this
/// crate.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ExchangeState {
    /// Rounds opened so far.
    pub rounds: u32,
}

impl ExchangeState {
    /// A fresh episode, with no round opened yet.
    #[must_use]
    pub const fn opened() -> Self {
        Self { rounds: 0 }
    }

    /// The state after opening one more round.
    ///
    /// Saturating, so a host that runs past `u32::MAX` rounds stops counting
    /// rather than wrapping back under its own cap.
    #[must_use]
    pub const fn advanced(self) -> Self {
        Self {
            rounds: self.rounds.saturating_add(1),
        }
    }
}

/// Why no exchange round is open.
///
/// A closed round always names a reason; there is no silent no-op, for the
/// same reason [`crate::hive::HiveStep::Idle`] is a variant rather than an empty
/// turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoExchangeReason {
    /// The host has not turned the mechanism on.
    Disabled,
    /// `round_cap` rounds have already opened.
    RoundsSpent,
    /// Every active member has spent its own `contact_cap`.
    ContactsSpent,
    /// The desk has nobody on it who could contact anybody else.
    TooFewMembers,
}

/// Whether members may exchange privately now, and which of them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ExchangeRound {
    /// Each of these members may append **at most one** private row now.
    ///
    /// In desk order, and containing only members that are active on the
    /// episode's desk and still under their own contact cap. A host may run
    /// any subset of them, in any order, including none.
    Open {
        /// The members this round authorizes, in desk order.
        members: Vec<String>,
        /// Private rows this episode may still write, across every member.
        ///
        /// `u64`, because it is a sum over members of a `u32` cap and a `u32`
        /// sum would saturate silently — reporting `u32::MAX` for a policy
        /// whose real capacity is larger, which is a worse answer than a big
        /// one.
        remaining: u64,
        /// The state to carry into the next call, having opened this round.
        next: ExchangeState,
    },
    /// No round, and why.
    Closed {
        /// What closed it.
        reason: NoExchangeReason,
    },
}
