//! What to read, and what came back.

use crate::{Conversation, SessionMessage};

/// Which conversations to read for a seat, as of when.
#[derive(Clone, Debug)]
pub struct ElsewhereQuery<'a> {
    /// The seat the rows are read as.
    pub seat: &'a str,
    /// Every conversation the seat is in, including the one it is taking a
    /// turn in: that one is skipped rather than having to be left out.
    pub conversations: &'a [Conversation],
    /// The conversation the turn is in, skipped. `None` reads them all,
    /// which is what a caller briefing a seat outside a turn wants.
    pub current: Option<&'a Conversation>,
    /// Exclusive upper bound on every read, so one turn's context is read
    /// as of one moment.
    pub before: Option<crate::Sequence>,
    /// Rows per conversation.
    pub window: usize,
}

/// One conversation's newest rows, as the seat reads them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Elsewhere {
    /// Which conversation.
    pub conversation: Conversation,
    /// Its rows, chronological, narrowed to what the seat may read.
    pub rows: Vec<SessionMessage>,
}
