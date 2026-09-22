//! One completion-driven episode as a host steps it: the desk, its
//! conversations, and the rules between them.
//!
//! [`CompletionDriver`] folds one episode and never runs a turn. A live
//! episode is more than one fold: the desk's, and a child episode for every
//! conversation an `ask` opens (ADR 0023). Between them sit rules the driver
//! cannot hold because they span both, or because they are about what a
//! turn did *not* do:
//!
//! - **Conversations.** An ask roots one at its row, with the seat asked as
//!   its participant. It runs ahead of desk turns, because it is what
//!   unblocks one. It concludes when the seat asked completes, or at a wall,
//!   or when nothing is due anywhere; its outcome is cross-posted to the
//!   asker as a private row, which releases the asker's hold.
//! - **Nudges** (ADR 0024). A desk seat that holds open work, ran for it and
//!   has been shown everything is told once, per assignment, and owed a turn.
//!   A seat asked that took its turn and did not answer is told once and
//!   owed a turn; a second silence stands.
//! - **Sorting.** What a wave said lands in the channel it belongs to. A
//!   broadcast or an ask made inside a conversation is desk work; anything
//!   but a post or a completion inside one is dropped.
//! - **Refusals.** A completion the ledger refuses is explained to the seat
//!   on the desk; a spent broadcast budget completes the seat with the work.
//! - **Walls.** Turns per conversation, and turns per episode.
//!
//! [`Conductor`] holds all of that as state and folds. It appends nothing:
//! every row is the host's, so it hands the host [`Step`]s -- a [`Note`] to
//! append, a [`Commit`] to append and report the sequence of, an [`Event`]
//! to log -- and takes the sequence back. One wave, from the host's side:
//!
//! 1. [`begin_wave`](Conductor::begin_wave): the nudges due, as steps.
//! 2. [`turns`](Conductor::turns): who runs, where, and from which row.
//! 3. For each turn, [`open_turn`](Conductor::open_turn) with the rows the
//!    host will show it: the [`EpisodeBrief`] for the prompt.
//! 4. Run the turns; [`record`](Conductor::record) what each called.
//! 5. [`step`](Conductor::step) until it returns nothing, appending each
//!    note, appending each commit and reporting its sequence through
//!    [`committed`](Conductor::committed), and logging each event.
//!
//! [`finished`](Conductor::finished) says when to stop.

mod child;
mod steps;
#[cfg(test)]
mod test;
mod wave;

use std::collections::{BTreeMap, BTreeSet};

use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind::{Conversation, Sequence};
use tinyhivemind_embed::RoutingPlan;
use tinyhivemind_hive::{CompletionEpisodeState, apply_completion};

use crate::driver::{BroadcastRouting, Channel, ConversationView, EpisodeBrief};
use crate::{BoundAgent, CompletionDriver, DriverState, Error, Result};
use child::{Child, Concluded};
pub use steps::{Commit, Event, Note, Refusal, Step, Turn};
use wave::Wave;

/// The walls a conducted episode runs inside.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConductPolicy {
    /// Turns a conversation may take before it concludes without an answer.
    pub child_turn_wall: u64,
    /// Turns the whole episode may take before it is abandoned.
    pub turn_wall: u64,
}

impl Default for ConductPolicy {
    fn default() -> Self {
        Self {
            child_turn_wall: 6,
            turn_wall: 60,
        }
    }
}

/// The door: what the desk is, who sits at it, and who starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Door {
    /// The chat every tool call names: the desk id.
    pub chat: String,
    /// The desk's name, for the episode's conversation record.
    pub desk_name: String,
    /// Every seat, in desk order.
    pub members: Vec<String>,
    /// The seats the door route starts; the rest are completed at once.
    pub starters: Vec<String>,
    /// The task's row: where the passed-over seats are completed.
    pub opened_at: Sequence,
}

/// Who the door route starts: the plan's seats, or `fallback` when routing
/// asked for clarification nobody is there to give.
#[must_use]
pub fn starters(plan: &RoutingPlan, fallback: &str) -> Vec<String> {
    match plan {
        RoutingPlan::One { responder_id, .. } | RoutingPlan::Fallback { responder_id, .. } => {
            vec![responder_id.clone()]
        }
        RoutingPlan::Hive {
            primary_id,
            invited_ids,
            ..
        } => std::iter::once(primary_id.clone())
            .chain(invited_ids.iter().cloned())
            .collect(),
        RoutingPlan::Clarify { .. } => vec![fallback.to_owned()],
    }
}

/// The desk episode, its conversations, and the rules between them.
pub struct Conductor<'a, A: BoundAgent> {
    driver: &'a CompletionDriver<'a, A>,
    routing: BroadcastRouting<'a>,
    chat: String,
    desk_name: String,
    policy: ConductPolicy,
    state: DriverState,
    children: BTreeMap<Sequence, Child>,
    concluded: Vec<Concluded>,
    /// How many concluded conversations each seat has been shown.
    shown: BTreeMap<String, usize>,
    /// The assignment each seat was last nudged for on the desk.
    desk_nudged: BTreeMap<String, Sequence>,
    turns: u64,
    waves: u64,
    discharged: u64,
    wave: Wave,
}

impl<A: BoundAgent> std::fmt::Debug for Conductor<'_, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conductor")
            .field("chat", &self.chat)
            .field("turns", &self.turns)
            .field("waves", &self.waves)
            .field("conversations", &self.children.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl<'a, A: BoundAgent> Conductor<'a, A> {
    /// Open the desk episode. Every member is seated so a handoff can reach
    /// any seat; the ones the door route passed over are completed at once
    /// at `opened_at`, the task's row -- idle, and reopened by any broadcast
    /// that finds them.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownStarter`] for a starter that is not a member, and
    /// [`Error::NoStarters`] for none at all; otherwise the episode refusing
    /// its members, or the driver its start.
    pub fn open(
        driver: &'a CompletionDriver<'a, A>,
        routing: BroadcastRouting<'a>,
        policy: ConductPolicy,
        door: Door,
    ) -> Result<Self> {
        if door.starters.is_empty() {
            return Err(Error::NoStarters);
        }
        if let Some(seat) = door
            .starters
            .iter()
            .find(|seat| !door.members.contains(seat))
        {
            return Err(Error::UnknownStarter { seat: seat.clone() });
        }
        let mut episode = CompletionEpisodeState::opened(
            Conversation {
                desk_id: door.chat.clone(),
                desk_name: door.desk_name.clone(),
                thread_root: None,
            },
            Sequence(0),
            door.members.iter().map(String::as_str),
        )?;
        for id in &door.members {
            if !door.starters.contains(id) {
                episode = apply_completion(&episode, id, door.opened_at)?;
            }
        }
        let state = driver.start(episode)?;
        Ok(Self {
            driver,
            routing,
            chat: door.chat,
            desk_name: door.desk_name,
            policy,
            state,
            children: BTreeMap::new(),
            concluded: Vec::new(),
            shown: BTreeMap::new(),
            desk_nudged: BTreeMap::new(),
            turns: 0,
            waves: 0,
            discharged: 0,
            wave: Wave::default(),
        })
    }

    /// Over: the desk is quiescent and no conversation is open.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.state.quiescent() && self.children.is_empty()
    }

    /// The desk episode's state.
    #[must_use]
    pub const fn state(&self) -> &DriverState {
        &self.state
    }

    /// The chat every tool call names.
    #[must_use]
    pub fn chat(&self) -> &str {
        &self.chat
    }

    /// Seat turns run so far.
    #[must_use]
    pub const fn turns_run(&self) -> u64 {
        self.turns
    }

    /// Waves proposed so far.
    #[must_use]
    pub const fn waves(&self) -> u64 {
        self.waves
    }

    /// Seats completed with their work for a spent broadcast budget.
    #[must_use]
    pub const fn discharged(&self) -> u64 {
        self.discharged
    }

    /// Conversations concluded so far.
    #[must_use]
    pub fn conversations(&self) -> usize {
        self.concluded.len()
    }

    /// Start a wave: the desk seats nothing will wake are told once, per
    /// assignment, and owed a turn.
    pub fn begin_wave(&mut self) -> Vec<Step> {
        self.waves += 1;
        let mut steps = Vec::new();
        for seat in self.state.stalled() {
            let assigned_at = self
                .state
                .episode()
                .participants
                .iter()
                .find(|participant| participant.agent_id == seat)
                .and_then(|participant| participant.open())
                .map(|record| record.assigned_at);
            if self.desk_nudged.get(&seat) == assigned_at.as_ref() {
                continue;
            }
            if let Some(at) = assigned_at {
                self.desk_nudged.insert(seat.clone(), at);
            }
            steps.push(Step::Event(Event::Nudged {
                seat: seat.clone(),
                thread: None,
            }));
            steps.push(Step::Note(Note {
                body: "you hold open work and nothing new has arrived. Call `complete_episode` \
                       with what you have, or `broadcast` the part that is another seat's. A \
                       reply without a tool call records nothing."
                    .to_owned(),
                thread: None,
                only_for: Some(seat.clone()),
            }));
            self.state.owe_turn(&seat);
        }
        for child in self.children.values_mut() {
            child.turned = false;
        }
        steps
    }

    /// The turns due this wave: one per seat, conversations first, because a
    /// conversation is what unblocks a desk turn.
    ///
    /// An empty wave with conversations open concludes them all, without an
    /// answer, in the steps that follow.
    ///
    /// # Errors
    ///
    /// [`Error::Stalled`]: nothing is due anywhere, no conversation is open,
    /// and the desk holds open work.
    pub fn turns(&mut self) -> Result<Vec<Turn>> {
        let mut taken: BTreeSet<String> = BTreeSet::new();
        let mut turns = Vec::new();
        for child in self.children.values() {
            for seat in self.pending(&child.state)? {
                if !taken.insert(seat.clone()) {
                    continue;
                }
                let since = child
                    .state
                    .seen()
                    .delivered_through
                    .get(&seat)
                    .copied()
                    .unwrap_or(child.root);
                turns.push(Turn {
                    channel: Channel::Thread {
                        root: child.root,
                        other: child.other(&seat),
                        opened_it: seat == child.asker,
                    },
                    seat,
                    since,
                });
            }
        }
        for seat in self.pending(&self.state)? {
            if !taken.insert(seat.clone()) {
                continue;
            }
            let since = self
                .state
                .seen()
                .delivered_through
                .get(&seat)
                .copied()
                .unwrap_or(Sequence(0));
            turns.push(Turn {
                seat,
                channel: Channel::Desk,
                since,
            });
        }
        if turns.is_empty() && self.children.is_empty() {
            return Err(Error::Stalled {
                seats: self.state.stalled(),
            });
        }
        self.wave.begin(turns.is_empty());
        Ok(turns)
    }

    fn pending(&self, state: &DriverState) -> Result<Vec<String>> {
        Ok(self
            .driver
            .pending_round(state)?
            .agents()
            .iter()
            .map(|pending| pending.hive_agent_id.to_owned())
            .collect())
    }

    /// Open a turn: record that the seat is shown everything through
    /// `latest`, and that it ran for what it holds, and build its brief.
    /// `new_rows` are the rows above [`Turn::since`] in the turn's channel,
    /// rendered by the host; `transcript` is any thread of the desk, whole,
    /// for the conversations the seat is or was in.
    pub fn open_turn(
        &mut self,
        turn: &Turn,
        latest: Sequence,
        new_rows: Vec<String>,
        mut transcript: impl FnMut(Sequence) -> Vec<String>,
    ) -> EpisodeBrief {
        match turn.channel {
            Channel::Thread { root, .. } => {
                if let Some(child) = self.children.get_mut(&root) {
                    child.state.delivered(&turn.seat, latest);
                    child.state.turn_started(&turn.seat);
                    child.turns += 1;
                    child.turned = true;
                    return EpisodeBrief::for_turn(
                        &child.state,
                        self.chat.clone(),
                        turn.seat.clone(),
                        turn.channel.clone(),
                        new_rows,
                        Vec::new(),
                    );
                }
                EpisodeBrief::for_turn(
                    &self.state,
                    self.chat.clone(),
                    turn.seat.clone(),
                    turn.channel.clone(),
                    new_rows,
                    Vec::new(),
                )
            }
            Channel::Desk => {
                self.state.delivered(&turn.seat, latest);
                self.state.turn_started(&turn.seat);
                let views = self.views(&turn.seat, &mut transcript);
                EpisodeBrief::for_turn(
                    &self.state,
                    self.chat.clone(),
                    turn.seat.clone(),
                    Channel::Desk,
                    new_rows,
                    views,
                )
            }
        }
    }

    /// The conversations a seat is shown on a desk turn: those concluded
    /// since it last spoke, whole, once; and any still in progress.
    fn views(
        &mut self,
        seat: &str,
        transcript: &mut impl FnMut(Sequence) -> Vec<String>,
    ) -> Vec<ConversationView> {
        let cursor = self.shown.entry(seat.to_owned()).or_insert(0);
        let mut views: Vec<ConversationView> = self.concluded[*cursor..]
            .iter()
            .filter(|done| done.involves(seat))
            .map(|done| done.view(seat, transcript(done.root)))
            .collect();
        *cursor = self.concluded.len();
        views.extend(
            self.children
                .values()
                .filter(|child| child.involves(seat))
                .map(|child| child.view(seat, transcript(child.root))),
        );
        views
    }

    /// What a turn called, in call order, once the host has closed it.
    /// Sorted into the channel each belongs to: a broadcast or an ask made
    /// inside a conversation is desk work.
    pub fn record(&mut self, turn: &Turn, calls: impl IntoIterator<Item = ToolCall>) {
        self.turns += 1;
        for call in calls {
            let ToolCall::Speak(utterance) = call else {
                continue;
            };
            match (turn.thread(), &utterance) {
                (None, _) | (Some(_), Utterance::Broadcast { .. } | Utterance::Ask { .. }) => {
                    self.wave.desk.push((turn.seat.clone(), utterance));
                }
                (Some(root), _) => self.wave.thread.push((root, turn.seat.clone(), utterance)),
            }
        }
    }
}
