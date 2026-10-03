//! Stable quorum inputs and standings.

use serde::{Deserialize, Serialize};

use crate::hive::trace::TopicId;
use crate::runtime::{Sequence, responder::Probability};

/// Admission threshold for one semantic worker-output evaluation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AdmissionPolicy {
    /// Highest violation probability still admitted to consensus.
    pub maximum_violation_probability: Probability,
}

/// One topic or abstention probability in an evaluated worker stance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TopicProbability {
    /// Topic receiving probability, or `null` for abstention.
    pub topic: Option<TopicId>,
    /// Probability assigned to this outcome.
    pub probability: Probability,
}

/// Typed semantic evaluation bound to one authored output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DecisionEvaluation {
    /// Sequence of the exact authored output evaluated.
    pub source_sequence: Sequence,
    /// Authenticated author id expected at that sequence.
    pub agent_id: String,
    /// Complete Choice distribution across topics plus optional abstention.
    pub stance: Vec<TopicProbability>,
    /// Normalized Score result from unsupported (zero) to direct evidence (one).
    pub evidence_quality: Probability,
    /// Noul probability that the output violates supplied constraints.
    pub violation_probability: Probability,
}

/// When the room is entitled to call a topic settled.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct QuorumPolicy {
    /// Distinct supporters a topic needs to carry.
    pub threshold: u32,
    /// How far back support still counts.
    ///
    /// The unit is whatever [`EpisodePolicy::distance`] says: raw sequences by
    /// default, or the rows the fold actually reads under [`Basis::Live`].
    /// The distinction matters on any desk that carries traffic this episode
    /// does not fold — an aside, an off-floor row, a reading from another
    /// channel — because under raw sequences all of it counts against this
    /// window.
    ///
    /// [`EpisodePolicy::distance`]: crate::hive::episode::EpisodePolicy::distance
    /// [`Basis::Live`]: crate::hive::horizon::Basis::Live
    pub window: u32,
    /// Whether support must cite grounds to count.
    pub require_grounded: bool,
}

impl QuorumPolicy {
    /// A conservative default: two grounded supporters within thirty sequences.
    ///
    pub const DEFAULT: Self = Self {
        threshold: 2,
        window: 30,
        require_grounded: true,
    };

    /// The policy a desk of `members` should carry.
    ///
    /// [`Self::DEFAULT`] is an absolute count and an absolute window, and both
    /// stop meaning what they say as the desk grows: two supporters carry a
    /// room of a thousand, and thirty sequences is a fraction of an episode
    /// that runs for three thousand turns. Neither failure announces itself —
    /// the room decides, or fails to, and nothing says the policy was the
    /// reason.
    ///
    /// The threshold is bounded on both sides, and the benchmark found both
    /// bounds rather than reasoning them out.
    ///
    /// *Above half the desk* is what suppresses deadlock. Two options that
    /// both clear the line are deadlocked by definition and no further support
    /// resolves it, so raising the line above half makes two *disjoint*
    /// supporter sets impossible — and the benchmark's measured deadlock rate
    /// falls to zero.
    ///
    /// **It does not make deadlock unreachable, and this is worth stating
    /// precisely.** The trace grammar lets one member back several topics, and
    /// a member in both supporter sets is not two members. Three members at a
    /// threshold of two deadlock the moment one of them advocates both
    /// options — `episode::test::deadlock::a_majority_threshold_does_not_make_deadlock_unreachable`
    /// is that case. What the benchmark measures is a room whose members each
    /// back one option; a host whose participants argue for two should expect
    /// [`ConsensusState::Deadlocked`] and handle it, not assume it away.
    ///
    /// *Below the whole desk* is what keeps a decision reachable:
    /// cross-inhibition removes a silenced advocate and never puts them back,
    /// so at unanimity one grounded `!object` ends the episode's chance of
    /// quorum. Between the two, the smallest majority that still leaves a
    /// member to spare.
    ///
    /// The window is the episode: support deposited in the opening round has
    /// to still count when the room settles, and an absolute window silently
    /// stops covering the episode somewhere above thirty turns. Pass the
    /// budget from [`EpisodePolicy::for_room`], which is what that constructor
    /// does.
    ///
    /// [`EpisodePolicy::for_room`]: crate::hive::episode::EpisodePolicy::for_room
    #[must_use]
    pub const fn for_room(members: u32, window: u32) -> Self {
        let majority = members / 2 + 1;
        let capped = if majority > members.saturating_sub(1) {
            members.saturating_sub(1)
        } else {
            majority
        };
        let threshold = if capped < 2 { 2 } else { capped };
        Self {
            threshold,
            window: if window == 0 { 1 } else { window },
            ..Self::DEFAULT
        }
    }
}

impl Default for QuorumPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Where one topic stands in the current window.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TopicStanding {
    /// The topic being weighed.
    pub topic: TopicId,
    /// Distinct still-counting supporters, in first-support order.
    pub supporters: Vec<String>,
    /// Advocates silenced by a grounded objection, in first-silenced order.
    pub silenced: Vec<String>,
    /// Distinct members who refuted this topic, in first-refutation order.
    ///
    /// Nothing is removed when a topic is refuted. It keeps its supporters and
    /// its weight and stays in the standings, so the transcript records that
    /// the room considered it and a reader can audit the refutation back to the
    /// message it cites.
    pub refuted_by: Vec<String>,
    /// Fixed-point weight of the surviving support.
    pub support: i64,
    /// Sum of admitted expected supporter contributions in parts per million.
    pub probability_support: u64,
}

impl TopicStanding {
    /// Return whether expected probability support reaches the scaled
    /// `policy.threshold`.
    #[must_use]
    pub fn carried(&self, policy: &QuorumPolicy) -> bool {
        self.probability_support
            >= u64::from(policy.threshold) * u64::from(super::PROBABILITY_SCALE)
    }
}

/// What the room's standings add up to.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConsensusState {
    /// No topic has carried yet.
    Deliberating,
    /// Exactly one topic has carried.
    Quorum {
        /// The topic that carried.
        topic: TopicId,
    },
    /// Two or more topics carried at once, in topic order.
    Deadlocked {
        /// Every topic at or above threshold.
        topics: Vec<TopicId>,
    },
}
