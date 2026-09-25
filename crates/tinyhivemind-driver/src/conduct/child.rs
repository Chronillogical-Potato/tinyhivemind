//! One open conversation: a thread of the desk, run as its own episode.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use tinyhivemind::Sequence;

use crate::driver::{ConversationView, DriverState};

/// A conversation rooted at an ask row, whose participants are the seats
/// asked, with the asker recorded here (ADR 0023, ADR 0026). One ask names
/// one or a group, and a group is **one** conversation, not one each: the
/// seats asked read each other and each concludes with `complete_episode`,
/// whose message is its answer. It is over when every one of them has, and a
/// follow-up is a further ask.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Child {
    pub(super) root: Sequence,
    pub(super) asker: String,
    /// The seats asked, in the order the ask named them. Never empty: an ask
    /// naming nobody is refused before a conversation is opened.
    pub(super) askees: Vec<String>,
    pub(super) state: DriverState,
    /// Turns taken in it so far.
    pub(super) turns: u64,
    /// The seats asked that have been told once they have not answered.
    #[serde(default)]
    pub(super) nudged: BTreeSet<String>,
    /// The seats asked that took a turn in this wave. Carried across a
    /// snapshot: a wave can be resumed mid-flight, and dropping this would
    /// lose the nudge owed to a seat that was asked and said nothing.
    #[serde(default)]
    pub(super) turned: BTreeSet<String>,
    /// The seats asked whose conclusion has reached the asker. The
    /// conversation closes once this is all of them; until then a conclusion
    /// the fold refused is re-issued for whoever is still missing.
    #[serde(default)]
    pub(super) answered: BTreeSet<String>,
}

impl Child {
    pub(super) fn new(root: Sequence, by: &str, to: &[String], state: DriverState) -> Self {
        Self {
            root,
            asker: by.to_owned(),
            askees: to.to_vec(),
            state,
            turns: 0,
            nudged: BTreeSet::new(),
            turned: BTreeSet::new(),
            answered: BTreeSet::new(),
        }
    }

    /// Whether `seat` is in it, on either side.
    pub(super) fn involves(&self, seat: &str) -> bool {
        self.asker == seat || self.askees.iter().any(|askee| askee == seat)
    }

    /// The other seats, from `seat`'s side: everyone it was asked with for a
    /// seat asked, and everyone asked for the seat that asked them.
    pub(super) fn others(&self, seat: &str) -> Vec<String> {
        if seat == self.asker {
            return self.askees.clone();
        }
        let mut others = vec![self.asker.clone()];
        others.extend(self.askees.iter().filter(|askee| *askee != seat).cloned());
        others
    }

    /// Whether a seat asked has yet to answer, and so may still be nudged.
    pub(super) fn owes_an_answer(&self, seat: &str) -> bool {
        self.state
            .episode()
            .participants
            .iter()
            .any(|participant| participant.agent_id == seat && participant.open().is_some())
    }

    /// Over: every seat asked completed, or the wall was reached.
    ///
    /// The wall is per seat asked, because it was sized for a conversation
    /// that one seat answers: a group of three gets three times the turns to
    /// reach the same one answer each, rather than a third of the room's
    /// question each.
    pub(super) fn is_over(&self, wall: u64) -> bool {
        self.state.quiescent() || self.turns >= wall.saturating_mul(self.askees.len() as u64)
    }

    /// How `seat` sees it, with the transcript the host holds.
    pub(super) fn view(&self, seat: &str, transcript: Vec<String>) -> ConversationView {
        ConversationView {
            root: self.root,
            others: self.others(seat),
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
    /// The seats asked. A snapshot from before a conversation could hold a
    /// group wrote one `askee`, which reads here as a group of one.
    #[serde(alias = "askee", deserialize_with = "one_or_many")]
    pub(super) askees: Vec<String>,
}

/// One seat or several: what `askee` was, and what `askees` is.
fn one_or_many<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Seats {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Seats::deserialize(deserializer)? {
        Seats::One(seat) => vec![seat],
        Seats::Many(seats) => seats,
    })
}

/// A set of seats, or the flag that stood for one before a conversation
/// could hold a group: `true` meant its single askee, `false` meant nobody.
#[derive(Deserialize)]
#[serde(untagged)]
enum Marked {
    Seats(BTreeSet<String>),
    Whoever(bool),
}

impl Marked {
    fn over(self, askees: &[String]) -> BTreeSet<String> {
        match self {
            Self::Seats(seats) => seats,
            Self::Whoever(true) => askees.iter().cloned().collect(),
            Self::Whoever(false) => BTreeSet::new(),
        }
    }
}

/// A conversation as a snapshot holds it, in either shape.
///
/// The fields a snapshot written before ADR 0026 carried -- one `askee`, and
/// `nudged`/`turned` as flags over that one seat -- are read here and mapped
/// forward, so a host resuming an episode in flight across this change
/// recovers it rather than failing to decode the conductor it checkpointed.
#[derive(Deserialize)]
struct Snapshotted {
    root: Sequence,
    asker: String,
    #[serde(alias = "askee", deserialize_with = "one_or_many")]
    askees: Vec<String>,
    state: DriverState,
    #[serde(default)]
    turns: u64,
    #[serde(default = "nobody")]
    nudged: Marked,
    #[serde(default = "nobody")]
    turned: Marked,
    #[serde(default)]
    answered: BTreeSet<String>,
}

const fn nobody() -> Marked {
    Marked::Seats(BTreeSet::new())
}

impl<'de> Deserialize<'de> for Child {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let held = Snapshotted::deserialize(deserializer)?;
        Ok(Self {
            root: held.root,
            asker: held.asker,
            nudged: held.nudged.over(&held.askees),
            turned: held.turned.over(&held.askees),
            askees: held.askees,
            state: held.state,
            turns: held.turns,
            answered: held.answered,
        })
    }
}

impl Concluded {
    pub(super) fn involves(&self, seat: &str) -> bool {
        self.asker == seat || self.askees.iter().any(|askee| askee == seat)
    }

    pub(super) fn view(&self, seat: &str, transcript: Vec<String>) -> ConversationView {
        let others = if seat == self.asker {
            self.askees.clone()
        } else {
            let mut others = vec![self.asker.clone()];
            others.extend(self.askees.iter().filter(|askee| *askee != seat).cloned());
            others
        };
        ConversationView {
            root: self.root,
            others,
            opened_it: seat == self.asker,
            transcript,
            concluded: true,
        }
    }
}
