//! What the driver remembers between committed events that the episode fold
//! does not: queued handoffs, broadcast budgets, open questions, and how far
//! each seat has been shown.
//!
//! `CompletionEpisodeState` records who holds an assignment and who completed
//! one. It has no record of a handoff that could not be given yet, of how many
//! broadcasts an assignment has already spent, or of a question whose answer is
//! still owed. Those live here, beside the episode, and the [`Ledger`] is
//! advanced only by the same committed events -- so a persisted `DriverState`
//! carries it, and a crash between a completion and its queue drain loses
//! nothing the episode itself never recorded.
//!
//! [`Seen`] is different in kind: it is what the *host* reports about each
//! seat -- what it was shown, what it ran for. It moves the wake predicate, not
//! the episode, and a pending round stays valid across it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use tinyhivemind::Sequence;
use tinyhivemind_hive::CompletionEpisodeState;

/// One routed handoff held for a recipient that was still working.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Handoff {
    /// Who broadcast it.
    pub from: String,
    /// The broadcast text, verbatim.
    pub body: String,
    /// The sequence of the broadcast row it came from.
    pub origin: Sequence,
}

/// Broadcasts charged against one assignment.
///
/// The budget resets by comparison rather than by sweep: a record whose
/// `assigned_at` is not the seat's current open assignment reads as unspent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AssignmentSpend {
    /// The assignment charged.
    pub assigned_at: Sequence,
    /// Broadcasts routed under it.
    pub broadcasts: u32,
}

/// State advanced only by committed events, kept beside the episode fold.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Ledger {
    /// Handoffs waiting on a busy recipient, oldest first.
    #[serde(default)]
    pub queues: BTreeMap<String, VecDeque<Handoff>>,
    /// Broadcast budgets, by author.
    #[serde(default)]
    pub spent: BTreeMap<String, AssignmentSpend>,
    /// Open conversations: each asker to the seats it awaits, and the sequence
    /// of the row that asked. A private message from an asked seat to the
    /// asker is the conclusion that releases it.
    #[serde(default)]
    pub outstanding_asks: BTreeMap<String, BTreeMap<String, Sequence>>,
}

impl Ledger {
    /// Broadcasts already charged to this seat's assignment at `assigned_at`.
    #[must_use]
    pub fn charged(&self, agent_id: &str, assigned_at: Sequence) -> u32 {
        self.spent
            .get(agent_id)
            .filter(|record| record.assigned_at == assigned_at)
            .map_or(0, |record| record.broadcasts)
    }

    pub(super) fn charge(&mut self, agent_id: &str, assigned_at: Sequence) {
        let broadcasts = self.charged(agent_id, assigned_at).saturating_add(1);
        self.spent.insert(
            agent_id.to_owned(),
            AssignmentSpend {
                assigned_at,
                broadcasts,
            },
        );
    }

    /// How many handoffs wait on this seat.
    #[must_use]
    pub fn queue_len(&self, agent_id: &str) -> usize {
        self.queues.get(agent_id).map_or(0, VecDeque::len)
    }

    pub(super) fn push(&mut self, agent_id: &str, handoff: Handoff) {
        self.queues
            .entry(agent_id.to_owned())
            .or_default()
            .push_back(handoff);
    }

    pub(super) fn pop(&mut self, agent_id: &str) -> Option<Handoff> {
        let handoff = self.queues.get_mut(agent_id)?.pop_front();
        if self.queue_len(agent_id) == 0 {
            self.queues.remove(agent_id);
        }
        handoff
    }

    /// The seats this one is still waiting on, with the row that asked each.
    #[must_use]
    pub fn awaiting(&self, agent_id: &str) -> Option<&BTreeMap<String, Sequence>> {
        self.outstanding_asks
            .get(agent_id)
            .filter(|asked| !asked.is_empty())
    }

    pub(super) fn open_ask(&mut self, asker: &str, seat: &str, at: Sequence) {
        self.outstanding_asks
            .entry(asker.to_owned())
            .or_default()
            .insert(seat.to_owned(), at);
    }

    /// A private message from `by` to `asker` is the answer `asker` awaited
    /// from `by`: the conclusion of the conversation between them.
    pub(super) fn answered(&mut self, by: &str, asker: &str) {
        if let Some(asked) = self.outstanding_asks.get_mut(asker) {
            asked.remove(by);
        }
        self.outstanding_asks.retain(|_, asked| !asked.is_empty());
    }

    /// The open asks that name `agent_id` as the seat owing an answer.
    pub(super) fn asked_of<'a>(&'a self, agent_id: &'a str) -> impl Iterator<Item = Sequence> + 'a {
        self.outstanding_asks
            .values()
            .filter_map(move |asked| asked.get(agent_id).copied())
    }

    /// Nothing queued and nothing awaited.
    #[must_use]
    pub fn is_drained(&self) -> bool {
        self.queues.values().all(VecDeque::is_empty) && self.outstanding_asks.is_empty()
    }
}

/// What the host has reported about each seat, outside the committed record.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Seen {
    /// The newest sequence each seat has actually been shown.
    #[serde(default)]
    pub delivered_through: BTreeMap<String, Sequence>,
    /// The assignment each seat last ran a turn for.
    ///
    /// Not persisted: after a restart nothing is running, and a seat that ran
    /// but never committed is owed its turn again -- one redundant turn is the
    /// safe direction to fail in.
    #[serde(skip)]
    pub ran_for: BTreeMap<String, Sequence>,
}

impl Seen {
    pub(super) fn delivered(&mut self, agent_id: &str, through: Sequence) {
        let entry = self
            .delivered_through
            .entry(agent_id.to_owned())
            .or_insert(through);
        if through > *entry {
            *entry = through;
        }
    }

    pub(super) fn ran(&mut self, agent_id: &str, assigned_at: Sequence) {
        self.ran_for.insert(agent_id.to_owned(), assigned_at);
    }
}

/// The sequence at which this seat's open assignment was given, if it has one.
pub(super) fn open_assignment(
    episode: &CompletionEpisodeState,
    agent_id: &str,
) -> Option<Sequence> {
    episode
        .participants
        .iter()
        .find(|participant| participant.agent_id == agent_id)
        .and_then(|participant| participant.open())
        .map(|record| record.assigned_at)
}

/// Every id the ledger names, for membership validation on resume.
pub(super) fn named_ids(ledger: &Ledger) -> BTreeSet<&str> {
    ledger
        .queues
        .keys()
        .chain(ledger.spent.keys())
        .chain(ledger.outstanding_asks.keys())
        .chain(ledger.outstanding_asks.values().flat_map(BTreeMap::keys))
        .chain(
            ledger
                .queues
                .values()
                .flatten()
                .map(|handoff| &handoff.from),
        )
        .map(String::as_str)
        .collect()
}
