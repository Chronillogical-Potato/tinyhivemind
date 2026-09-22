//! After a wave: what the seats said, committed in order, and what follows.
//!
//! The host appends every row, so this is a phase machine the host steps:
//! [`Conductor::step`] hands out one [`Step`] at a time, and a [`Commit`]
//! is not followed by another step until the host has reported its
//! sequence through [`Conductor::committed`]. The phases, in order: what
//! was said in conversations; the seats asked that said nothing; what was
//! said on the desk, with its consequences; the conversations that are
//! over; the turn wall.

use std::collections::VecDeque;

use tinyhivemind::Sequence;
use tinyhivemind::speech::Utterance;
use tinyhivemind_hive::CompletionEpisodeState;

use super::Conductor;
use super::child::{Child, Concluded};
use super::steps::{Commit, Event, Kind, Note, Refusal, Step};
use crate::driver::{BroadcastRouting, CommittedUtterance, HostAction, Transition};
use tinyhivemind::Conversation;

use crate::{BoundAgent, Error, Result};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    /// No wave in progress.
    #[default]
    Idle,
    /// Commit what was said in conversations.
    Threads,
    /// Tell the seats asked that said nothing.
    SilentAskees,
    /// Commit what was said on the desk.
    Desk,
    /// Conclude the conversations that are over.
    Conclude,
    /// Check the turn wall.
    Wall,
}

/// One wave's bookkeeping.
#[derive(Debug, Default)]
pub(super) struct Wave {
    phase: Phase,
    /// Nothing was due: every open conversation concludes without an answer.
    force_conclusions: bool,
    /// What thread turns said: `(root, seat, utterance)`.
    pub(super) thread: Vec<(Sequence, String, Utterance)>,
    /// What desk turns said, and what thread turns said to the desk:
    /// `(seat, utterance, the conversation it was lifted out of)`.
    pub(super) desk: Vec<(String, Utterance, Option<Sequence>)>,
    /// Steps ready for the host, notes and events.
    steps: VecDeque<Step>,
    /// Commits waiting for the host, in order.
    commits: VecDeque<Commit>,
    /// The commit the host holds and has not reported.
    outstanding: Option<Commit>,
}

impl Wave {
    pub(super) fn begin(&mut self, nothing_due: bool) {
        self.phase = Phase::Threads;
        self.force_conclusions = nothing_due;
    }

    fn event(&mut self, event: Event) {
        self.steps.push_back(Step::Event(event));
    }

    fn note(&mut self, body: impl Into<String>, thread: Option<Sequence>, only_for: Option<&str>) {
        self.steps.push_back(Step::Note(Note {
            body: body.into(),
            thread,
            only_for: only_for.map(str::to_owned),
        }));
    }
}

impl<'a, A: BoundAgent> Conductor<'a, A> {
    /// The next step after a wave, or `None` when the wave is settled.
    ///
    /// # Errors
    ///
    /// [`Error::CommitOutstanding`] when the last commit's sequence has not
    /// been reported; [`Error::TurnWall`] when the episode has run past it.
    pub fn step(&mut self) -> Result<Option<Step>> {
        loop {
            if let Some(step) = self.wave.steps.pop_front() {
                return Ok(Some(step));
            }
            if self.wave.outstanding.is_some() {
                return Err(Error::CommitOutstanding);
            }
            if let Some(commit) = self.wave.commits.pop_front() {
                self.wave.outstanding = Some(commit.clone());
                return Ok(Some(Step::Commit(commit)));
            }
            match self.wave.phase {
                Phase::Idle => return Ok(None),
                Phase::Threads => {
                    for (root, seat, utterance) in std::mem::take(&mut self.wave.thread) {
                        self.queue_thread(root, seat, utterance);
                    }
                    self.wave.phase = Phase::SilentAskees;
                }
                Phase::SilentAskees => {
                    // Nothing due means every conversation concludes now; a
                    // nudge would owe a turn nobody will run.
                    if !self.wave.force_conclusions {
                        self.nudge_silent_askees();
                    }
                    self.wave.phase = Phase::Desk;
                }
                Phase::Desk => {
                    for (seat, utterance, conversation) in std::mem::take(&mut self.wave.desk) {
                        let only_for = utterance.asks().map(str::to_owned);
                        self.wave.commits.push_back(Commit {
                            author: seat,
                            utterance,
                            thread: None,
                            only_for,
                            conversation,
                            kind: Kind::Desk,
                        });
                    }
                    self.wave.phase = Phase::Conclude;
                }
                Phase::Conclude => {
                    self.queue_conclusions();
                    self.wave.phase = Phase::Wall;
                }
                Phase::Wall => {
                    self.wave.phase = Phase::Idle;
                    if self.turns >= self.policy.turn_wall {
                        return Err(Error::TurnWall {
                            wall: self.policy.turn_wall,
                        });
                    }
                }
            }
        }
    }

    /// A row committed to a conversation. Only a post or a completion can be
    /// said inside one; `dm` is not served, and the rest went to the desk.
    fn queue_thread(&mut self, root: Sequence, seat: String, utterance: Utterance) {
        if !self.children.contains_key(&root)
            || !matches!(
                utterance,
                Utterance::Post { .. } | Utterance::CompleteEpisode { .. }
            )
        {
            return;
        }
        self.wave.commits.push_back(Commit {
            author: seat,
            utterance,
            thread: Some(root),
            only_for: None,
            conversation: Some(root),
            kind: Kind::Thread { root },
        });
    }

    /// The seat asked took its turn and the conversation is not over: it did
    /// not answer, whatever it did instead. Once, it is told so and owed one
    /// more turn; a second silence stands. A conversation at its wall is
    /// concluding this wave and is not nudged.
    fn nudge_silent_askees(&mut self) {
        let wall = self.policy.child_turn_wall;
        for child in self.children.values_mut() {
            if child.turned && !child.is_over(wall) && !child.nudged {
                child.nudged = true;
                self.wave.steps.push_back(Step::Note(Note {
                    body: "the seat that asked you is waiting: answer with `complete_episode`, \
                           and its message is your answer. If you need another seat first, say \
                           so in that answer."
                        .to_owned(),
                    thread: Some(child.root),
                    only_for: None,
                }));
                child.state.owe_turn(&child.askee);
                self.wave.steps.push_back(Step::Event(Event::Nudged {
                    seat: child.askee.clone(),
                    thread: Some(child.root),
                }));
            }
        }
    }

    /// Conversations that ended this wave, or ran past their wall, or were
    /// left with nothing due anywhere, conclude: their outcome is
    /// cross-posted to the asker, which releases its hold.
    fn queue_conclusions(&mut self) {
        let wall = self.policy.child_turn_wall;
        let force = self.wave.force_conclusions;
        let over: Vec<Sequence> = self
            .children
            .iter()
            .filter(|(_, child)| force || child.is_over(wall))
            .map(|(root, _)| *root)
            .collect();
        for root in over {
            let Some(child) = self.children.get(&root) else {
                continue;
            };
            let forced = !child.state.quiescent();
            self.wave.commits.push_back(Commit {
                author: child.askee.clone(),
                utterance: Utterance::Dm {
                    to: vec![child.asker.clone()],
                    message: format!(
                        "concluded our conversation (thread {}): {}",
                        root.0,
                        child.outcome(forced)
                    ),
                },
                thread: None,
                only_for: Some(child.asker.clone()),
                conversation: Some(root),
                kind: Kind::Conclusion { root, forced },
            });
        }
    }

    /// The sequence the host gave the outstanding commit.
    ///
    /// # Errors
    ///
    /// [`Error::NoCommitOutstanding`] when nothing was handed out, or any
    /// driver error the fold could not explain to the seat.
    pub async fn committed(&mut self, sequence: Sequence) -> Result<()> {
        let commit = self
            .wave
            .outstanding
            .take()
            .ok_or(Error::NoCommitOutstanding)?;
        let committed = CommittedUtterance {
            author_id: commit.author.clone(),
            sequence,
            utterance: commit.utterance.clone(),
        };
        match commit.kind {
            Kind::Thread { root } => self.commit_thread(root, committed).await,
            Kind::Desk => self.commit_desk(committed).await,
            Kind::Conclusion { root, forced } => {
                self.commit_conclusion(root, forced, committed).await
            }
            Kind::Discharge => {
                let transition = self
                    .driver
                    .apply_committed(&self.state, committed, None)
                    .await?;
                self.state = transition.state;
                Ok(())
            }
        }
    }

    async fn commit_thread(&mut self, root: Sequence, committed: CommittedUtterance) -> Result<()> {
        let Some(child) = self.children.get_mut(&root) else {
            return Ok(());
        };
        let seat = committed.author_id.clone();
        let at = committed.sequence;
        let said = committed.utterance.message().to_owned();
        match self
            .driver
            .apply_committed(&child.state, committed, None)
            .await
        {
            Ok(transition) => {
                child.state = transition.state;
                // The answer is what the fold accepted, not what was tried.
                if seat == child.askee {
                    child.last_by_askee = Some(said);
                }
            }
            Err(Error::UndeliveredAssignment { .. }) => self.wave.event(Event::Refused {
                seat,
                thread: Some(root),
                why: Refusal::NotYetShown,
                at,
            }),
            Err(error) => return Err(error),
        }
        Ok(())
    }

    fn routing(&self) -> BroadcastRouting<'a> {
        self.routing
    }

    async fn commit_desk(&mut self, committed: CommittedUtterance) -> Result<()> {
        let seat = committed.author_id.clone();
        let sequence = committed.sequence;
        let asked = committed.utterance.asks().map(str::to_owned);
        let is_broadcast = committed.utterance.broadcasting();
        let held = holds(&self.state, &seat);
        let routing = self.routing();
        match self
            .driver
            .apply_committed(&self.state, committed, Some(routing))
            .await
        {
            Ok(transition) => {
                let said = Said {
                    seat: seat.clone(),
                    sequence,
                    asked,
                    is_broadcast,
                    held,
                };
                self.consequences(&said, transition)?;
            }
            Err(Error::AwaitingReply { waiting_on, .. }) => {
                self.wave.note(
                    format!(
                        "your completion was refused: your conversation with {} has not \
                         concluded. Its outcome reaches you on a later turn; complete after it \
                         does.",
                        waiting_on.join(", ")
                    ),
                    None,
                    Some(&seat),
                );
                self.wave.event(Event::Refused {
                    seat,
                    thread: None,
                    why: Refusal::AwaitingReply { waiting_on },
                    at: sequence,
                });
            }
            Err(Error::UndeliveredAssignment { assigned_at, .. }) => {
                self.wave.note(
                    format!(
                        "you were handed new work at sequence {} while you were speaking; it is \
                         in your next messages. Your completion applied to nothing.",
                        assigned_at.0
                    ),
                    None,
                    Some(&seat),
                );
                self.wave.event(Event::Refused {
                    seat,
                    thread: None,
                    why: Refusal::Undelivered { assigned_at },
                    at: sequence,
                });
            }
            Err(Error::BudgetSpent { .. }) => {
                self.discharged += 1;
                self.wave.event(Event::Discharged {
                    seat: seat.clone(),
                    at: sequence,
                });
                // Before whatever else the wave said: the seat keeps the work
                // now, so a later row from it applies to that.
                self.wave.commits.push_front(Commit {
                    author: seat,
                    utterance: Utterance::CompleteEpisode {
                        message: "budget spent; keeping the work".into(),
                    },
                    thread: None,
                    only_for: None,
                    conversation: None,
                    kind: Kind::Discharge,
                });
            }
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// What follows from a desk row the fold accepted: broadcasts placed or
    /// not, a conversation opened, handoffs delivered.
    fn consequences(&mut self, said: &Said, transition: Transition) -> Result<()> {
        let mut routed = false;
        for action in transition.actions {
            match action {
                HostAction::RunAgents { agent_ids, .. } => {
                    routed = true;
                    self.wave.event(Event::Broadcast {
                        seat: said.seat.clone(),
                        to: agent_ids,
                        at: said.sequence,
                    });
                }
                // For an ask, this is the signal to open the conversation: a
                // thread of the desk rooted at the ask row, with the two as
                // its seats.
                HostAction::DeliverDm { .. } => {
                    if let Some(askee) = &said.asked {
                        self.open_conversation(&said.seat, askee, said.sequence)?;
                    }
                }
                HostAction::DeliverHandoff { agent_id, handoff } => {
                    self.wave.event(Event::Handoff {
                        to: agent_id.clone(),
                        from: handoff.from.clone(),
                        origin: handoff.origin,
                    });
                    self.wave.note(
                        format!("handoff from @{}: {}", handoff.from, handoff.body),
                        None,
                        Some(&agent_id),
                    );
                }
            }
        }
        if said.is_broadcast && said.held && !holds(&transition.state, &said.seat) {
            self.wave.event(Event::CompletedByBroadcast {
                seat: said.seat.clone(),
                at: said.sequence,
            });
        }
        if said.is_broadcast && !routed {
            self.wave.event(Event::Unplaced {
                seat: said.seat.clone(),
                at: said.sequence,
            });
            self.wave.note(
                "nobody on this desk can take that; the work stays with you. Do what you can \
                 with what the desk holds, or complete with what you have.",
                None,
                Some(&said.seat),
            );
        }
        self.state = transition.state;
        Ok(())
    }

    fn open_conversation(&mut self, by: &str, to: &str, root: Sequence) -> Result<()> {
        let state = self.driver.start(CompletionEpisodeState::opened(
            Conversation {
                desk_id: self.chat.clone(),
                desk_name: self.desk_name.clone(),
                thread_root: Some(root),
            },
            root,
            [to],
        )?)?;
        self.wave.event(Event::Asked {
            seat: by.to_owned(),
            askee: to.to_owned(),
            root,
        });
        self.children.insert(root, Child::new(root, by, to, state));
        Ok(())
    }

    async fn commit_conclusion(
        &mut self,
        root: Sequence,
        forced: bool,
        committed: CommittedUtterance,
    ) -> Result<()> {
        let Some(child) = self.children.remove(&root) else {
            return Ok(());
        };
        let at = committed.sequence;
        let transition = self
            .driver
            .apply_committed(&self.state, committed, None)
            .await?;
        self.state = transition.state;
        self.wave.event(Event::Concluded {
            root,
            asker: child.asker.clone(),
            askee: child.askee.clone(),
            forced,
            at,
        });
        self.concluded.push(Concluded {
            root,
            asker: child.asker,
            askee: child.askee,
        });
        Ok(())
    }
}

/// A desk row as it was, before the fold moved anything.
struct Said {
    seat: String,
    sequence: Sequence,
    asked: Option<String>,
    is_broadcast: bool,
    held: bool,
}

/// Whether `seat` holds an open assignment on the desk.
fn holds(state: &crate::DriverState, seat: &str) -> bool {
    state
        .episode()
        .participants
        .iter()
        .any(|participant| participant.agent_id == seat && participant.open().is_some())
}
