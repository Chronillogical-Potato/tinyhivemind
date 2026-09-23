//! One open conversation: a thread of the desk, run as its own episode.

use serde::{Deserialize, Serialize};
use tinyhivemind::Sequence;

use crate::driver::{ConversationView, DriverState};

/// A conversation rooted at an ask row, whose participant is the seat asked,
/// with the asker recorded here (ADR 0023). One question, one answer: the
/// seat asked concludes with `complete_episode`, and its message is the
/// answer; a follow-up is a further ask.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Child {
    pub(super) root: Sequence,
    pub(super) asker: String,
    pub(super) askee: String,
    pub(super) state: DriverState,
    /// Turns taken in it so far.
    pub(super) turns: u64,
    /// The last thing the seat asked said in it: the conclusion, cross-posted.
    pub(super) last_by_askee: Option<String>,
    /// Whether the seat asked has been told once that it has not answered.
    pub(super) nudged: bool,
    /// Whether the seat asked took a turn in this wave. Carried across a
    /// snapshot: a wave can be resumed mid-flight, and dropping this would
    /// lose the nudge owed to a seat that was asked and said nothing.
    pub(super) turned: bool,
}

impl Child {
    pub(super) fn new(root: Sequence, by: &str, to: &str, state: DriverState) -> Self {
        Self {
            root,
            asker: by.to_owned(),
            askee: to.to_owned(),
            state,
            turns: 0,
            last_by_askee: None,
            nudged: false,
            turned: false,
        }
    }

    /// Whether `seat` is one of its two.
    pub(super) fn involves(&self, seat: &str) -> bool {
        self.asker == seat || self.askee == seat
    }

    /// The other seat, from `seat`'s side.
    pub(super) fn other(&self, seat: &str) -> String {
        if seat == self.asker {
            self.askee.clone()
        } else {
            self.asker.clone()
        }
    }

    /// Over: the seat asked completed, or the wall was reached.
    pub(super) fn is_over(&self, wall: u64) -> bool {
        self.state.quiescent() || self.turns >= wall
    }

    /// What reaches the asker when it concludes.
    pub(super) fn outcome(&self, forced: bool) -> String {
        if forced {
            "the conversation did not conclude in time; take what was said and proceed".to_owned()
        } else {
            self.last_by_askee
                .clone()
                .unwrap_or_else(|| "concluded".to_owned())
        }
    }

    /// How `seat` sees it, with the transcript the host holds.
    pub(super) fn view(&self, seat: &str, transcript: Vec<String>) -> ConversationView {
        ConversationView {
            root: self.root,
            other: self.other(seat),
            opened_it: seat == self.asker,
            transcript,
            concluded: false,
        }
    }
}

/// A conversation that concluded, kept for the context of the seats that had
/// it: shown whole once to each, on its next desk turn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct Concluded {
    pub(super) root: Sequence,
    pub(super) asker: String,
    pub(super) askee: String,
}

impl Concluded {
    pub(super) fn involves(&self, seat: &str) -> bool {
        self.asker == seat || self.askee == seat
    }

    pub(super) fn view(&self, seat: &str, transcript: Vec<String>) -> ConversationView {
        ConversationView {
            root: self.root,
            other: if seat == self.asker {
                self.askee.clone()
            } else {
                self.asker.clone()
            },
            opened_it: seat == self.asker,
            transcript,
            concluded: true,
        }
    }
}
