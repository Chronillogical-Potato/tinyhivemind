//! Typed adapter failures.

use tinyhivemind::Sequence;

/// A malformed graph, route, or committed completion event.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A canonical identity is empty or whitespace-only.
    #[error("{collection} contains a blank canonical id")]
    BlankCanonicalId {
        /// Graph collection containing the invalid id.
        collection: &'static str,
    },
    /// A canonical identity uses the semantic router's reserved alternative.
    #[error("{collection} contains reserved canonical id `none`")]
    ReservedCanonicalId {
        /// Graph collection containing the reserved id.
        collection: &'static str,
    },
    /// The desk snapshot failed `TinyHiveMind` validation.
    #[error(transparent)]
    InvalidDesk(#[from] tinyhivemind_core::error::Error),
    /// A desk has no members and therefore cannot run a hive.
    #[error("desk `{desk_id}` has no members")]
    EmptyMembership {
        /// Empty desk id.
        desk_id: String,
    },
    /// The desk membership repeats a canonical id.
    #[error("desk member `{agent_id}` appears more than once")]
    DuplicateMemberId {
        /// Repeated canonical id.
        agent_id: String,
    },
    /// Two route candidates have the same canonical id.
    #[error("route candidate `{agent_id}` appears more than once")]
    DuplicateCandidateId {
        /// Repeated canonical id.
        agent_id: String,
    },
    /// Two bindings have the same canonical id.
    #[error("agent binding `{agent_id}` appears more than once")]
    DuplicateBindingId {
        /// Repeated canonical id.
        agent_id: String,
    },
    /// A graph collection does not equal the desk membership set.
    #[error("{collection} ids do not equal desk membership")]
    MembershipMismatch {
        /// Collection that failed equality: `candidates` or `bindings`.
        collection: &'static str,
        /// Desk members missing from the collection.
        missing: Vec<String>,
        /// Collection ids that are not desk members.
        outsiders: Vec<String>,
    },
    /// An accepted plan names an agent outside this hive.
    #[error("unknown bound agent `{agent_id}`")]
    UnknownBoundAgent {
        /// Unknown canonical id.
        agent_id: String,
    },
    /// A broadcast route attempted to send work back to its author.
    #[error("broadcast route includes its author `{agent_id}`")]
    BroadcastIncludesAuthor {
        /// Invalid routed author id.
        agent_id: String,
    },
    /// A sender is not a member of this hive.
    #[error("dm sender `{agent_id}` is not a member of the desk")]
    UnknownDmSender {
        /// Invalid sender id.
        agent_id: String,
    },
    /// A DM recipient is not a member of this hive.
    #[error("dm recipient `{agent_id}` is not a member of the desk")]
    UnknownDmRecipient {
        /// Invalid recipient id.
        agent_id: String,
    },
    /// A DM names no recipient.
    #[error("dm must name at least one recipient")]
    EmptyDmRecipients,
    /// A DM repeats one recipient.
    #[error("dm recipient `{agent_id}` appears more than once")]
    DuplicateDmRecipient {
        /// Repeated recipient id.
        agent_id: String,
    },
    /// A private message exceeds the configured round width.
    #[error("dm has {recipient_count} recipients but round width is {round_width}")]
    DmTooWide {
        /// Number of requested recipients.
        recipient_count: usize,
        /// Configured maximum.
        round_width: usize,
    },
    /// A semantic broadcast route exceeded the driver's configured bound.
    #[error("broadcast has {recipient_count} recipients but round width is {round_width}")]
    BroadcastTooWide {
        /// Number of routed recipients.
        recipient_count: usize,
        /// Driver maximum.
        round_width: usize,
    },
    /// The requested pending-round width is zero.
    #[error("pending round width must not be zero")]
    ZeroRoundWidth,
    /// A committed broadcast was folded without routing inputs.
    #[error("committed broadcast requires routing inputs")]
    MissingBroadcastRouting,
    /// A broadcast author is the episode's only participant.
    #[error("broadcast author `{agent_id}` has no other episode participant for fallback")]
    NoBroadcastFallback {
        /// Author for whom no distinct fallback exists.
        agent_id: String,
    },
    /// Completion state belongs to another desk.
    #[error("completion episode is outside desk `{desk_id}`")]
    OutOfHiveEpisode {
        /// Hive desk id.
        desk_id: String,
    },
    /// A completion participant is outside the hive.
    #[error("completion participant `{agent_id}` is outside the hive")]
    OutOfHiveParticipant {
        /// Invalid participant id.
        agent_id: String,
    },
    /// A committed event sequence is not after the episode watermark.
    #[error("stale committed event at sequence {sequence}")]
    StaleCommittedEvent {
        /// Rejected host sequence.
        sequence: Sequence,
    },
    /// A sequence was already committed with different event content.
    #[error("sequence {sequence} is already committed to another event")]
    DuplicateCommittedSequence {
        /// Conflicting host sequence.
        sequence: Sequence,
    },
    /// A serialized receipt's map key disagrees with its committed event.
    #[error("receipt key {stored} does not match event sequence {event}")]
    InvalidReceiptSequence {
        /// Sequence used as the receipt map key.
        stored: Sequence,
        /// Sequence carried by the committed event.
        event: Sequence,
    },
    /// A serialized state's revision disagrees with its committed receipts.
    #[error("driver state revision {revision} does not match {receipt_count} receipts")]
    InvalidStateRevision {
        /// Serialized monotonic revision.
        revision: u64,
        /// Number of committed receipts represented by the state.
        receipt_count: usize,
    },
    /// A serialized freshness floor disagrees with its episode and receipts.
    #[error("driver freshness floor {stored} does not match derived floor {derived}")]
    InvalidFreshnessFloor {
        /// Serialized maximum accepted or episode-owned sequence.
        stored: Sequence,
        /// Maximum derived from the episode and committed receipts.
        derived: Sequence,
    },
    /// The monotonic driver-state revision cannot be advanced further.
    #[error("driver state revision overflow")]
    StateRevisionOverflow,
    /// A pending round was proposed from an older driver state.
    #[error("pending round revision {round_revision} is stale for state revision {state_revision}")]
    StaleRound {
        /// Revision from which the round was proposed.
        round_revision: u64,
        /// Revision of the state receiving the round.
        state_revision: u64,
    },
    /// A pending round belongs to a different driver-state snapshot.
    #[error(
        "pending round revision {round_revision} does not match state revision {state_revision}"
    )]
    MismatchedRound {
        /// Revision from which the round was proposed.
        round_revision: u64,
        /// Revision of the state receiving the round.
        state_revision: u64,
    },
    /// Only a prefix of a pending round was reported as committed.
    #[error("committed round is partial: expected {expected}, received {received}")]
    PartialRound {
        /// Number of turns proposed.
        expected: usize,
        /// Number of committed results supplied.
        received: usize,
    },
    /// A committed round result came from an agent outside its proposal.
    #[error("committed round includes unproposed agent `{agent_id}`")]
    UnexpectedRoundAuthor {
        /// Unexpected canonical id.
        agent_id: String,
    },
    /// A caller-supplied desk routing request disagrees with the hive graph.
    #[error("desk routing request `{field}` does not match the hive graph")]
    MismatchedDeskRoutingRequest {
        /// Request field that disagreed with the graph.
        field: &'static str,
    },
    /// A seat tried to complete while a question it asked is unanswered.
    #[error("participant `{agent_id}` is still waiting on {waiting_on:?} and may not complete")]
    AwaitingReply {
        /// The seat that asked.
        agent_id: String,
        /// The seats whose answers it still awaits.
        waiting_on: Vec<String>,
    },
    /// A seat tried to complete an assignment the host never showed it.
    #[error(
        "participant `{agent_id}` was assigned at {assigned_at} but delivered only through {delivered_through}"
    )]
    UndeliveredAssignment {
        /// The seat completing.
        agent_id: String,
        /// Where its open assignment sits.
        assigned_at: Sequence,
        /// The newest row the host reports having shown it.
        delivered_through: Sequence,
    },
    /// A seat has spent every broadcast its current assignment allows.
    #[error(
        "participant `{agent_id}` has spent its broadcast budget for the assignment at {assigned_at}"
    )]
    BudgetSpent {
        /// The author refused.
        agent_id: String,
        /// The assignment whose budget is gone.
        assigned_at: Sequence,
    },
    /// The per-recipient handoff queue bound is zero.
    #[error("handoff queue depth must not be zero")]
    ZeroQueueDepth,
    /// The underlying completion fold rejected the event.
    #[error(transparent)]
    Completion(#[from] tinyhivemind_hive::Error),
    /// Nothing is due anywhere, no conversation is open, and the desk holds
    /// open work: nobody is owed a turn, so nothing will move.
    #[error("episode stalled with {seats:?} holding open work")]
    Stalled {
        /// The seats holding it.
        seats: Vec<String>,
    },
    /// Nothing is due anywhere and every seat that could move is parked on
    /// the host, which released none of them.
    #[error("episode parked on {seats:?} and the host released none")]
    Parked {
        /// The seats parked.
        seats: Vec<String>,
    },
    /// The episode ran past its turn wall.
    #[error("turn wall of {wall} reached")]
    TurnWall {
        /// The wall.
        wall: u64,
    },
    /// The host asked for the next step while holding a commit it has not
    /// reported the sequence of.
    #[error("a commit is outstanding; report its sequence first")]
    CommitOutstanding,
    /// The host reported a sequence for a commit it was never handed.
    #[error("no commit is outstanding")]
    NoCommitOutstanding,
    /// The door named a starter that is not a member of the desk.
    #[error("starter `{seat}` is not a member of the desk")]
    UnknownStarter {
        /// The seat.
        seat: String,
    },
    /// The door named nobody to start.
    #[error("the door names no starter")]
    NoStarters,
}

/// The crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;
