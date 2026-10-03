//! The simulated participant: its state and what it can be told about a
//! turn's payload.
//!
//! [`SimAgent`] itself is defined here so [`Room`](super::Room) and the rest
//! of `sim` have one place to name the type. Its own `impl` is split across
//! two descendant modules by what each method is *for*: [`state`] holds the
//! methods that construct and mutate a member's holdings, and [`turn`] holds
//! the methods that decide what one turn says -- including the
//! [`Participant`](crate::run::Participant) wrapper that lets the host drive
//! it. Splitting by concern rather than declaring one `impl SimAgent` block
//! per file is what a single 2,400-line module would otherwise force.

use tinyhivemind_core::hive::trace::TopicId;
use tinyhivemind_core::hive::{QuorumPolicy, Sequence};

use crate::context::{ContextBudget, ContextEntry};
use crate::rng::Rng;

use super::Role;

mod state;
mod turn;

/// A participant that holds a private, noisy view of every option.
#[derive(Clone, Debug)]
pub(crate) struct SimAgent {
    /// Canonical agent id.
    pub(crate) id: String,
    /// How it fills a turn with no strong move.
    pub(crate) role: Role,
    /// Private evaluation per topic, aligned with [`Room::topics`](super::Room).
    ///
    /// `pub(crate)` because [`Room::pooled`](super::Room::pooled) and
    /// [`Room::pre_checked`](super::Room::pre_checked) read one member's own
    /// evaluations to hand to a peer, and
    /// [`View::better_than_floor`](super::view::View::better_than_floor)
    /// reads them to find what this member now rates highest.
    pub(crate) evals: Vec<(TopicId, i32)>,
    /// Readings of a topic that arrived from outside this member's own desk,
    /// as a running sum and a count.
    ///
    /// A member that hears another channel's reading of an option does not
    /// replace its own with it and does not simply defer to it: it averages
    /// the two, which is the whole operation a channel boundary otherwise
    /// prevents. Nothing here touches the supporter sets — an imported reading
    /// changes what a member *believes*, and it still has to spend a turn
    /// saying so before the room counts it.
    ///
    /// `pub(crate)` so the self-check in [`super::view`] can assert on it
    /// directly, the same way it asserts on every other property a check
    /// answer is defined to have.
    pub(crate) imports: Vec<(TopicId, i64, u32)>,
    /// Everything this member has been told, in arrival order, one row each.
    ///
    /// Parallel to `imports` and `ruled_out` rather than replacing them: under
    /// [`ContextBudget::UNBOUNDED`] nothing reads this and the arm is
    /// bit-identical to one built before the window existed.
    context: Vec<ContextEntry>,
    /// How much of the above this member can still read.
    budget: ContextBudget,
    /// Its own argmax over `evals`.
    ///
    /// `pub(crate)` for [`Room::resampled`](super::Room::resampled) and the
    /// self-check in [`super::view`], which both read a member's favourite
    /// directly rather than through the [`Self::favourite`] accessor.
    pub(crate) favourite: TopicId,
    /// Drives noncompliance only; never the private evaluations.
    ///
    /// `pub(crate)` because [`Room::resampled`](super::Room::resampled)
    /// reseeds it directly, from outside this module.
    pub(crate) rng: Rng,
    /// The room's quorum rule, which is public: a participant is entitled to
    /// know how many grounded supporters settle a question, and reads the
    /// medium through the library's own fold rather than a private imitation
    /// of it.
    quorum: QuorumPolicy,
    /// The topic this member specialises in, under `Expertise::Specialists`.
    /// `None` under every other shape.
    ///
    /// `pub(crate)` so room generation, in [`super::generation`], can set it,
    /// and [`View::deferrable`](super::view::View::deferrable) can read it.
    pub(crate) specialty: Option<TopicId>,
    /// The topic this member holds a refuting fact for, under
    /// `Expertise::HiddenProfile`. `None` for every other member and shape.
    ///
    /// `pub(crate)` for the same reason `specialty` is, and so
    /// [`Room::pooled`](super::Room::pooled) can read it off a peer.
    pub(crate) refutes: Option<TopicId>,
    /// What this member's own turn costs, charged by the vote arm and summed
    /// into a deliberation's `cost_units`. `1` unless `Room::generate_with`
    /// was asked for `cost_tiers` and this member is a specialist.
    ///
    /// `pub(crate)` so [`Room::at_cost`](super::Room::at_cost) and room
    /// generation can set it directly.
    pub(crate) cost_unit: u32,
    /// Whether this member opens with a deposit rather than a position, and
    /// puts its own best option on the floor rather than backing a worse one
    /// somebody else got there first with. See the module docs: it is a
    /// participant policy, off unless `--blind-evidence` asked for it.
    blind_evidence: bool,
    /// Turns this member may spend asking one peer for its reading before
    /// committing to a position. `0` turns the move off, which is what every
    /// arm before the aside arms passes.
    aside_cap: u32,
    /// Checks this member has already opened.
    asides_spent: u32,
    /// What this member's checks do beyond costing a turn: where they are
    /// aimed, whether an answer may carry a fact, and whether the answer is
    /// taken in at all.
    style: CheckStyle,
    /// Options a private exchange has told this member are ruled out. Read by
    /// [`Self::score`], and by nothing the room counts.
    ///
    /// `pub(crate)` so the self-check in [`super::view`] can assert on it
    /// directly.
    pub(crate) ruled_out: Vec<TopicId>,
    /// The other members of this member's desk, for a continuous exchange
    /// that has to name a peer before it has seen one speak.
    ///
    /// Read only by [`CheckStyle::exchange`]. Every other arm finds its peer
    /// in the transcript, which is what confines those arms to the turns
    /// after the blind round.
    peers: Vec<String>,
    /// Peers this member has already contacted under
    /// [`CheckStyle::exchange`], so a continuous exchange reaches each of them
    /// once rather than the same one repeatedly.
    contacted: Vec<String>,
    /// Whether the turn this member has just composed was a real contribution
    /// rather than [`NONCOMPLIANCE`](super::NONCOMPLIANCE) filler.
    ///
    /// Read only by the alongside arm, so a member drifting through a turn
    /// does not also open a private check on it. Without it the two halves of
    /// the same experiment would differ in a second way: an on-floor check
    /// never happens on a noncompliant turn either, because `compose` returns
    /// before reaching one.
    complied: bool,
    /// Sequences of exchanges this member has already answered or folded in,
    /// so neither is done twice.
    handled: Vec<Sequence>,
}

/// One member's private evaluations and the fact it holds, if any.
///
/// What a peer could ever hand over, read once out of the room so the
/// zero-cost arms do not clone a participant per contact.
///
/// `pub(super)` because [`Room::pooled`](super::Room::pooled) and
/// [`Room::pre_checked`](super::Room::pre_checked), in the parent module,
/// build a `Vec` of these before installing them into the room's members.
pub(super) type Holdings = (Vec<(TopicId, i32)>, Option<TopicId>);

/// What one answered check hands over.
///
/// Three of these vary the *content* of an exchange while leaving its turns,
/// its words and its audience alone, which is what makes the arms that use
/// them a matched set rather than four unrelated experiments.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Payload {
    /// The responder's own reading of the one option it was asked about.
    #[default]
    Reading,
    /// That, and the fact that rules the option out when the responder holds
    /// one — what the room's public `!evidence` grammar has always carried.
    Fact,
    /// Nothing at all: the answer is written and discarded. The matched-turn
    /// control, whose loss against `hive+` is what the turns cost.
    Nothing,
    /// A reading of *every* option, and any fact — what a contact transfers in
    /// the biology, and what only a row that costs no turn can afford.
    Everything,
    /// [`Payload::Everything`] written and then thrown away by its reader.
    ///
    /// Byte-for-byte the same rows on the same rounds, consuming the same
    /// sequence numbers, transferring nothing. The control that separates what
    /// an off-floor exchange *says* from what merely writing its rows does to
    /// a fold that reads recency off raw sequence distance.
    Discarded,
}

/// What one arm's pairwise check does, beyond costing a turn.
///
/// Bundled rather than passed as loose flags so a call site says which knob it
/// is turning. [`CheckStyle::PLAIN`] is the original move: aimed at whoever
/// spoke first, carrying a reading, in place of the member's floor turn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CheckStyle {
    /// Aim the question at whoever the room has heard ground the option.
    pub(crate) informed: bool,
    /// Send the check **alongside** the member's floor move rather than in
    /// place of it, so the exchange costs the room no turn at all.
    pub(crate) alongside: bool,
    /// What an answer hands over.
    pub(crate) payload: Payload,
}

impl CheckStyle {
    /// Aimed at whoever spoke first, carrying a reading, taken in, on the
    /// floor.
    pub(crate) const PLAIN: Self = Self {
        informed: false,
        alongside: false,
        payload: Payload::Reading,
    };
    /// Aimed at whoever the room has heard ground the option.
    pub(crate) const AIMED: Self = Self {
        informed: true,
        alongside: false,
        payload: Payload::Reading,
    };
    /// Aimed, and carrying the fact rather than a number.
    pub(crate) const FACT: Self = Self {
        informed: true,
        alongside: false,
        payload: Payload::Fact,
    };
    /// The same turns, transferring nothing.
    pub(crate) const MUTE: Self = Self {
        informed: false,
        alongside: false,
        payload: Payload::Nothing,
    };
    /// Aimed, carrying the fact, and riding alongside the floor move rather
    /// than replacing it. The mechanism the scheduling result points at.
    pub(crate) const ALONGSIDE: Self = Self {
        informed: true,
        alongside: true,
        payload: Payload::Fact,
    };
    /// The same free row, spent continuously and carrying everything the donor
    /// holds rather than one answer to one question.
    pub(crate) const EXCHANGE: Self = Self {
        informed: false,
        alongside: true,
        payload: Payload::Everything,
    };
    /// The same rows on the same rounds, with every answer discarded.
    ///
    /// The control an off-floor exchange needs and an on-floor one does not.
    /// A private row consumes a sequence number, and `salience::standing`
    /// reads recency as a raw sequence distance — so writing twenty of them an
    /// episode perturbs which member the attention market hands the floor to,
    /// whatever they say. This arm writes them and says nothing, so whatever
    /// it moves is that perturbation rather than information.
    pub(crate) const QUIET: Self = Self {
        informed: false,
        alongside: true,
        payload: Payload::Discarded,
    };

    /// Whether an answer may carry the fact that rules an option out.
    pub(crate) const fn evidence(self) -> bool {
        matches!(self.payload, Payload::Fact | Payload::Everything)
    }

    /// Whether an answer is discarded rather than taken in.
    pub(crate) const fn mute(self) -> bool {
        matches!(self.payload, Payload::Nothing | Payload::Discarded)
    }

    /// Whether a contact happens every turn and carries every option.
    ///
    /// True of [`Payload::Discarded`] as well as [`Payload::Everything`]: the
    /// control has to *write* the same rows to be a control, and differs only
    /// in what its reader does with them.
    pub(crate) const fn exchange(self) -> bool {
        matches!(self.payload, Payload::Everything | Payload::Discarded)
    }
}
