//! What passes between the conductor and its host: a turn to run, and the
//! steps the host takes on the conductor's behalf after a wave.
//!
//! Every type here is a wire form. A host journals commits and events and
//! streams them to whatever renders the desk, so the serde representation is
//! pinned by a unit test: internally tagged, `snake_case`, and every field a
//! host can act on present by name.

use serde::{Deserialize, Serialize};
use tinyhivemind::Sequence;
use tinyhivemind::speech::Utterance;

use crate::driver::Channel;

/// One turn the host runs this wave.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Turn {
    /// The seat.
    pub seat: String,
    /// Where the turn runs: the desk, or a conversation on one of its threads.
    pub channel: Channel,
    /// The newest row the seat has been shown in this channel, or `None`
    /// for a seat shown nothing there yet: the host gives the turn every
    /// row above it, which for `None` is every row. A sequence is never
    /// borrowed to mean "nothing": a host may number its first row zero.
    /// On the wire the field is present, `null` for `None`.
    #[serde(deserialize_with = "required_null")]
    pub since: Option<Sequence>,
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Commit {
    /// The seat the row is attributed to.
    pub author: String,
    /// What was said.
    pub utterance: Utterance,
    /// The thread it lands in, or `None` for the open desk.
    pub thread: Option<Sequence>,
    /// On the open desk, the one seat it reaches; `None` reaches every seat.
    pub only_for: Option<String>,
    /// The conversation this row belongs to, by the ask row it is rooted at.
    ///
    /// Set for a row said inside a conversation, for desk work a seat lifted
    /// out of one -- a broadcast or an ask made while talking, which lands on
    /// the desk with no `thread` -- and for the row that concludes one to its
    /// asker. `None` for everything said on the desk itself, including the
    /// ask that opens a conversation: that row's own sequence is the root.
    /// A host shows one agent-to-agent exchange whole by taking the ask row
    /// and every row whose `conversation` is its sequence, wherever they
    /// landed.
    pub conversation: Option<Sequence>,
    /// What the conductor does with the row once it has its sequence. Opaque
    /// to a host: carried so a commit survives the wire whole.
    #[serde(rename = "purpose")]
    pub(super) kind: Kind,
}

/// What the conductor does with a commit once it has its sequence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Kind {
    /// A seat spoke in a conversation.
    Thread { root: Sequence },
    /// A seat spoke on the desk; a broadcast is routed.
    Desk,
    /// A conversation concluded: its outcome, cross-posted to the asker.
    Conclusion { root: Sequence, forced: bool },
    /// A seat that spent its broadcast budget keeps the work.
    Discharge,
}

/// Why a seat's row was refused, in the terms the desk tells it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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

/// Something the episode did that a host may want to show. Nothing here
/// needs acting on; every consequence is already a [`Note`] or a [`Commit`].
///
/// An event about a row names it by `at`, the sequence the host gave that
/// row, so a host attaches the event to the row it drew rather than
/// inferring it from order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// A seat was told once that nothing will wake it.
    Nudged {
        /// The seat.
        seat: String,
        /// The thread, or `None` on the desk.
        thread: Option<Sequence>,
    },
    /// A seat's turn stopped on something only the host can settle, and the
    /// seat is held: not nudged, not stalled, not proposed, until the host
    /// releases it.
    Parked {
        /// The seat.
        seat: String,
        /// The thread, or `None` on the desk.
        thread: Option<Sequence>,
    },
    /// The host released a parked seat: it is owed a turn where it parked.
    Resumed {
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
        /// The broadcast row.
        at: Sequence,
    },
    /// A broadcast fit no seat; the author keeps the work.
    Unplaced {
        /// The author.
        seat: String,
        /// The broadcast row.
        at: Sequence,
    },
    /// A broadcast closed its author's own assignment.
    CompletedByBroadcast {
        /// The author.
        seat: String,
        /// The broadcast row.
        at: Sequence,
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
        /// The broadcast row it came from.
        origin: Sequence,
    },
    /// A row was refused, and the seat told why. The row is already on the
    /// host's journal; this is what marks it refused.
    Refused {
        /// The seat.
        seat: String,
        /// The thread, or `None` on the desk.
        thread: Option<Sequence>,
        /// Why.
        why: Refusal,
        /// The refused row.
        at: Sequence,
    },
    /// A seat spent its broadcast budget and was completed with the work.
    Discharged {
        /// The seat.
        seat: String,
        /// The broadcast row that was over budget.
        at: Sequence,
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
        /// The row that carried the outcome to the asker.
        at: Sequence,
    },
}

/// One step the host takes after a wave, in order.
///
/// On the wire, tagged by `step`, with the step's own fields beside the tag.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// Append this, attributed to the desk.
    Note(Note),
    /// Append this and report its sequence.
    Commit(Commit),
    /// Show this, or don't.
    Event(Event),
}

/// Deserialize a nullable field that must be present: `serde` fills a
/// missing `Option` with `None` by default, and a wire form that dropped
/// the field would then pass as one that sent `null`.
fn required_null<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
