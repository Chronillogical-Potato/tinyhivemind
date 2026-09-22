//! What passes between the conductor and its host: a turn to run, and the
//! steps the host takes on the conductor's behalf after a wave.

use tinyhivemind::Sequence;
use tinyhivemind::speech::Utterance;

use crate::driver::Channel;

/// One turn the host runs this wave.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Turn {
    /// The seat.
    pub seat: String,
    /// Where the turn runs: the desk, or a conversation on one of its threads.
    pub channel: Channel,
    /// The newest row the seat has been shown in this channel: the host
    /// gives the turn every row above it.
    pub since: Sequence,
}

impl Turn {
    /// The thread the turn runs in, or `None` on the desk.
    #[must_use]
    pub fn thread(&self) -> Option<Sequence> {
        match &self.channel {
            Channel::Desk => None,
            Channel::Thread { root, .. } => Some(*root),
        }
    }
}

/// A row the desk says to a seat: the host appends it, attributed to the
/// desk, and nothing is committed back. The wording is the episode's.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Note {
    /// What the desk says.
    pub body: String,
    /// The thread it lands in, or `None` for the open desk.
    pub thread: Option<Sequence>,
    /// On the open desk, the one seat it reaches; `None` reaches every seat.
    pub only_for: Option<String>,
}

/// A row the host appends and then commits back with the sequence it got:
/// what a seat said, or what the episode says on a seat's behalf.
///
/// The host renders the utterance as its own desk row and calls
/// [`Conductor::committed`](super::Conductor::committed) with the sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commit {
    /// The seat the row is attributed to.
    pub author: String,
    /// What was said.
    pub utterance: Utterance,
    /// The thread it lands in, or `None` for the open desk.
    pub thread: Option<Sequence>,
    /// On the open desk, the one seat it reaches; `None` reaches every seat.
    pub only_for: Option<String>,
    pub(super) kind: Kind,
}

/// What the conductor does with a commit once it has its sequence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    /// A seat spoke in a conversation.
    Thread(Sequence),
    /// A seat spoke on the desk; a broadcast is routed.
    Desk,
    /// A conversation concluded: its outcome, cross-posted to the asker.
    Conclusion { root: Sequence, forced: bool },
    /// A seat that spent its broadcast budget keeps the work.
    Discharge,
}

/// Why a seat's row was refused, in the terms the desk tells it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// It may not complete: the seats it asked have not answered.
    AwaitingReply {
        /// The seats it awaits.
        waiting_on: Vec<String>,
    },
    /// It completed before seeing the assignment it holds.
    Undelivered {
        /// Where that assignment sits.
        assigned_at: Sequence,
    },
    /// It spoke in a conversation it has not yet been shown.
    NotYetShown,
}

/// Something the episode did that a host may want to log. Nothing here needs
/// acting on; every consequence is already a [`Note`] or a [`Commit`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// A seat was told once that nothing will wake it.
    Nudged {
        /// The seat.
        seat: String,
        /// The thread, or `None` on the desk.
        thread: Option<Sequence>,
    },
    /// A broadcast was placed.
    Broadcast {
        /// The author.
        seat: String,
        /// Who took it.
        to: Vec<String>,
    },
    /// A broadcast fit no seat; the author keeps the work.
    Unplaced {
        /// The author.
        seat: String,
    },
    /// A broadcast closed its author's own assignment.
    CompletedByBroadcast {
        /// The author.
        seat: String,
    },
    /// An ask opened a conversation.
    Asked {
        /// The seat that asked.
        seat: String,
        /// The seat asked.
        askee: String,
        /// The ask row the conversation is rooted at.
        root: Sequence,
    },
    /// A queued handoff reached its recipient.
    Handoff {
        /// The recipient.
        to: String,
        /// The author of the broadcast it came from.
        from: String,
    },
    /// A row was refused, and the seat told why.
    Refused {
        /// The seat.
        seat: String,
        /// The thread, or `None` on the desk.
        thread: Option<Sequence>,
        /// Why.
        why: Refusal,
    },
    /// A seat spent its broadcast budget and was completed with the work.
    Discharged {
        /// The seat.
        seat: String,
    },
    /// A conversation concluded.
    Concluded {
        /// The ask row it was rooted at.
        root: Sequence,
        /// The seat that asked.
        asker: String,
        /// The seat asked.
        askee: String,
        /// Without an answer: nothing was due, or it ran out of turns.
        forced: bool,
    },
}

/// One step the host takes after a wave, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Step {
    /// Append this, attributed to the desk.
    Note(Note),
    /// Append this and report its sequence.
    Commit(Commit),
    /// Log this, or don't.
    Event(Event),
}
