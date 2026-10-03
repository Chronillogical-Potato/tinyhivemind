//! Hive mind mechanics: stigmergy, decaying salience, quorum sensing,
//! cross-inhibition, and a response-threshold bid for the floor.
//!
//! `hive` module adds the one thing a shared desk cannot do on its own:
//! **converge**. A message today selects exactly one responder off a
//! deterministic ladder, that agent replies, and the interaction ends. This
//! crate lets a room of agents put proposals side by side, accumulate support,
//! register a grounded objection, and terminate for a reason it can name.
//!
//! # One task, one agent. Two or more facets, one seat each
//!
//! The benchmark in this repository is specific about when a room is worth its
//! price, and the answer is **not** "when the task is long". On a task that
//! merely gets longer, a single agent compacting by a superseding account beat
//! a room of five at every horizon and every window, at a seventh of the
//! depth. On a task that gets *wider* — several independent sub-decisions at
//! once, each wanting different attention — the ordering flips, with the
//! crossover at two facets:
//!
//! ```text
//! all-facets %       F=1      F=2      F=4      F=8
//! one agent         78.7     56.9     28.1      7.8
//! one seat each     78.7     61.1     35.9     13.0
//! ```
//!
//! [`crate::hive::divide`] is that shape, and [`crate::hive::DivisionPolicy::DEFAULT`] is **on** —
//! the only default in this crate that is, because it is the only mechanism
//! here that was measured beating the best single agent the benchmark could
//! build. A task of one facet divides into one seat answering alone, which is
//! the rule falling out of the general case rather than a branch beside it.
//!
//! See `docs/adr/0015-the-division-of-labour-is-the-default-shape.md`.
//!
//! # Two episode modes
//!
//! [`crate::hive::completion`] tracks persistent agents doing assigned work. It advances
//! only when an agent explicitly calls `complete_episode` or an accepted
//! semantic broadcast assigns new work. It does not infer completion from
//! prose, quorum, or a turn count.
//!
//! The deliberation episode below remains the mode for reaching a decision by
//! quorum.
//!
//! ## A deliberation episode is a sequence of bounded rounds
//!
//! [`crate::hive::step`] is what decides one *facet* when its owner cannot decide it alone.
//! It is not deprecated by the division and is kept as the arm the division was
//! measured against.
//!
//! A hive mind is normally built as fan-out — publish a task, wake N agents,
//! gather the replies — and the failure of that shape is that nothing bounds
//! it. [`crate::hive::HiveStep::Speak`] carries a **round**: the [`crate::hive::HiveTurn`]s authorized to
//! run concurrently, at most [`crate::hive::EpisodePolicy::round_width`] of them while the
//! round is blind or [`crate::hive::EpisodePolicy::revealed_width`] of them once it is
//! revealed, plus the one [`crate::hive::EpisodeState`] the episode takes once all of them
//! are appended. The bound is the invariant; the serialization it replaced
//! never was.
//!
//! Independence is still bought as [`crate::hive::Visibility`] rather than hoped for, and a
//! round strengthens it rather than threatening it: members writing at the same
//! time cannot read each other, so **a concurrent round is a blind round**.
//! That matters because conformity in a group of models rises with interaction
//! time and with sight of a peer's position — this repository measures the
//! blind opening at 24 points — so a wide round is *less* correlated than the
//! same turns taken in series, not more.
//!
//! `round_width: 1` is the sequential episode and reproduces every number
//! recorded before rounds existed, bit for bit. See
//! `docs/specs/concurrent-rounds.md` and
//! `docs/adr/0014-a-round-authorizes-concurrent-turns.md`, which supersedes
//! `docs/adr/0002-hive-episodes-are-sequential.md` on the terms that ADR set.
//!
//! # Sizing a policy to the room
//!
//! [`crate::hive::EpisodePolicy::DEFAULT`] is written for a room of about five, and four of
//! its numbers are absolute where the quantity they bound scales with the desk:
//! a budget of twelve turns, a quorum of two supporters, a decay half-life of
//! twenty rows, and a blind round four turns wide. The first three fail
//! **without saying so** above about a dozen members — most sharply the budget,
//! because a room with more members than turns never completes its blind
//! opening round and so never sees itself at all. The fourth costs depth rather
//! than accuracy: widening a blind round is free by [`crate::hive::DEFAULT_ROUND_WIDTH`]'s
//! own argument, and the free width is the size of the room.
//!
//! [`crate::hive::EpisodePolicy::for_room`] derives all four from the size of the desk, and
//! is what a host with a real roster should use.
//!
//! # What this crate deliberately does not hold
//!
//! - **A port.** There is no trait here for a host to implement. An episode is
//!   [`crate::hive::step`], a fold over a transcript the caller already holds, and the host
//!   does its waiting through host-owned storage and scheduling.
//! - **Storage.** [`crate::hive::EpisodeState`] is returned, never applied. The caller
//!   commits it after its turn is durably appended.
//! - **Floating point.** Every score is fixed-point integer, so every payload
//!   derives [`std::cmp::Eq`] and every fold is reproducible.
//! - **A quality claim.** This is a protocol for bounded deliberation. Nothing
//!   here is shown to make answers better, and almost every positive
//!   multi-agent result in the literature is confounded by compute.
//!
//!
//! # Modules
//!
//! - [`crate::hive::attention`] — the bid each member makes for the floor, and the argmax.
//! - [`mod@crate::hive::directory`] — who knows what, folded from grounded deposits and the
//!   citations they drew.
//! - [`crate::hive::division`] — a task's facets, split across the seats that own them, and
//!   what each owner reads. The one mechanism here whose default is *on*.
//! - [`crate::hive::episode`] — the pure state machine, and the visibility filter.
//! - [`crate::hive::error`] — typed failures from malformed inputs.
//! - [`crate::hive::horizon`] — where a fold is measured to, and whether distance counts
//!   every row the host wrote or only the rows the fold reads.
//! - [`crate::hive::quorum`] — standings, cross-inhibition, and the consensus predicate.
//! - [`mod@crate::hive::salience`] — recency decay, importance, and relevance.
//! - [`crate::hive::trace`] — the stigmergic grammar and its read.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_core::runtime::{SessionAuthor, SessionMessage, Sequence, aside::Audience};
//! use tinyhivemind_core::hive::{
//!     quorum::{consensus, standings, ConsensusState, QuorumPolicy},
//!     trace::read,
//! };
//!
//! fn agent(id: &str) -> SessionAuthor {
//!     SessionAuthor::Agent { id: id.into(), label: id.into() }
//! }
//! fn said(sequence: u64, author: SessionAuthor, content: &str) -> SessionMessage {
//!     SessionMessage {
//!         sequence: Sequence(sequence),
//!         author,
//!         content: content.into(),
//!         audience: Audience::Desk,
//!         elided: None,
//!     }
//! }
//!
//! // Two agents propose; a third grounds its support in the first proposal.
//! let transcript = [
//!     said(1, agent("planner"), "!propose #stage Stage the rollout."),
//!     said(2, agent("scout"), "!propose #ship Ship it all at once."),
//!     said(3, agent("critic"), "!support #stage ^1 Staging bounds the blast radius."),
//!     said(4, agent("planner"), "!support #stage ^1 Agreed, and it is reversible."),
//! ];
//!
//! let traces = read(&transcript);
//! let policy = QuorumPolicy { window: 100, ..QuorumPolicy::DEFAULT };
//! let settled = standings(&traces, Sequence(4), &policy)?;
//!
//! // Two distinct grounded supporters carry `stage`; `ship` has none.
//! assert_eq!(consensus(&settled, &policy), ConsensusState::Quorum { topic: "stage".into() });
//!
//! // Refutations remain on the standing for audit; they do not alter quorum.
//! let mut contested = transcript.to_vec();
//! contested.extend([
//!     said(5, agent("auditor"), "!evidence Staging needs a second environment we do not have."),
//!     said(6, agent("auditor"), "!refute #stage ^5 There is nowhere to stage it."),
//!     said(7, agent("scout"), "!refute #stage ^5 Confirmed, the environment was retired."),
//! ]);
//! let contested_transcript = contested.clone();
//! let contested = standings(&read(&contested), Sequence(7), &policy)?;
//! assert_eq!(consensus(&contested, &policy), ConsensusState::Quorum { topic: "stage".into() });
//! assert_eq!(contested[0].refuted_by, ["auditor", "scout"]);
//! assert_eq!(contested[0].supporters, ["planner", "critic"]);
//!
//! // The same transcript also says who *knows* what. The auditor stated the
//! // fact and two members built on it, so the directory names the auditor on
//! // `stage` rather than the planner who merely advocated it.
//! use tinyhivemind_core::hive::directory::{directory, DirectoryPolicy};
//!
//! let policy = DirectoryPolicy { window: 100, ..DirectoryPolicy::DEFAULT };
//! let known = directory(&read(&contested_transcript), Sequence(7), &policy, &[])?;
//! assert_eq!(
//!     known.top(&"stage".into()).map(|entry| entry.agent_id.as_str()),
//!     Some("auditor"),
//! );
//! assert!(known.knows("auditor", &"stage".into(), &policy));
//! # Ok::<(), tinyhivemind_core::hive::error::Error>(())
//! ```

pub mod attention;
pub mod completion;
pub mod directory;
pub mod division;
pub mod episode;
pub mod error;
pub mod exchange;
pub mod horizon;
pub mod quorum;
pub mod salience;
pub mod trace;

pub use attention::{AgentThreshold, Bid, BidReason, bids, floor_holder, floor_round};
pub use completion::{
    AssignmentRecord, CompletionEpisodeState, CompletionStep, ParticipantCompletion,
    apply_assignment, apply_completion, status as completion_status,
};
pub use directory::{Directory, DirectoryEntry, DirectoryPolicy, WEIGHT_CEILING, directory};
pub use division::{Assignment, Division, DivisionPolicy, OwnerReason, divide};
pub use episode::{
    DEFAULT_REVEALED_WIDTH, DEFAULT_ROUND_WIDTH, EpisodePolicy, EpisodeState, HiveStep, HiveTurn,
    Phase, Visibility, project_for, step, step_with_evaluations,
};
pub use error::{Error, Result};
pub use exchange::{ExchangePolicy, ExchangeRound, ExchangeState, NoExchangeReason, exchange};
pub use horizon::{Basis, Horizon};
pub use quorum::{
    AdmissionPolicy, ConsensusState, DecisionEvaluation, PROBABILITY_SCALE, QuorumPolicy,
    TopicProbability, TopicStanding, consensus, standings, standings_with_evaluations,
};
pub use salience::{Salience, SalienceWeights, salience};
pub use trace::{TRACE_CAP, TopicId, Trace, TraceKind, read, resolve};
// A host that wants group deliberation takes this crate alone and gets the
// session runtime and the pure algebra with it, so the types it hands to
// `step` are the *same* types rather than structural twins. This crate's own
// `error`, `Error` and `Result` deliberately shadow the runtime's.

pub use crate::runtime::*;
