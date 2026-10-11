//! One floor per desk, and the referrals that are the only way between them.
//!
//! Each desk runs core's `CompletionDriver` over a `BoundHive` of its own
//! members: rounds of at most the routing policy's width, broadcasts routed by
//! core (with [`MentionRouter`](super::router::MentionRouter) as its router),
//! and an episode that ends when every assignment is complete. Messages cross
//! desks through core's `referral` fold and nowhere else, as
//! `crates/tinyhivemind-lang/examples/hive_view/desks.rs` does for the
//! simulated swarm. The host adds what `ReferralPolicy` leaves to a host:
//!
//! - **Links.** A referral travels only along a declared, directed `topology`
//!   or `referrals` link; a return travels back along its forward's link.
//!   Anything else is refused `unlinked`.
//! - **Reports.** When a desk's episode ends with no question outstanding,
//!   its report is referred down each outgoing `topology` link, at the hop that
//!   reached the desk plus one, and opens an episode there with every member
//!   assigned (a blind first round). The last declared desk's report is the
//!   hive's answer.
//! - **Asks.** `hive_refer` is a forward referral at hop zero; core lands it
//!   on the target desk's first eligible member, whose episode's report is
//!   carried home by core's `Return` to the seat that asked.
//! - **Width.** A desk may ask at most [`ASK_CAP`] times in a run: a stated
//!   constant, as the referral module's README asks of a host.

use std::collections::{BTreeMap, VecDeque};

use serde::Serialize;
use tinyhivemind_core::desk::{Desk, DeskSet};
use tinyhivemind_core::dispatch::{DispatchConversation, DispatchKey};
use tinyhivemind_core::driver::{
    AgentBinding, BoundAgent, BoundHive, BroadcastRouting, CommittedUtterance, CompletionDriver,
    DriverState, HiveGraph, HostAction,
};
use tinyhivemind_core::embed::RoutingPolicy;
use tinyhivemind_core::hive::{
    CompletionEpisodeState, CompletionStep, apply_assignment, completion_status,
};
use tinyhivemind_core::mention::{MentionAuthor, resolve as mentions_in};
use tinyhivemind_core::referral::{
    ReferralDecision, ReferralInput, ReferralKind, ReferralOrigin, ReferralPolicy, referral,
};
use tinyhivemind_core::roster::{Roster, RosterMember};
use tinyhivemind_core::runtime::speech::Utterance;
use tinyhivemind_core::runtime::{Conversation, Sequence};
use tinyhivemind_lang::LoweredHive;

use super::frames::{ReferralFrame, StandingFrame};
use super::router::MentionRouter;
use super::tools::{Action, Verdict};

/// Asks one desk may refer to other desks across a run.
pub(super) const ASK_CAP: u32 = 3;

/// A seat as the driver binds it: the hive id is the runtime agent id.
#[derive(Clone, Debug)]
pub(super) struct SeatHandle(pub(super) String);

impl BoundAgent for SeatHandle {
    fn runtime_id(&self) -> &str {
        &self.0
    }
}

/// One row of a desk's journal.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Row {
    pub(super) sequence: u64,
    pub(super) author: String,
    /// `opening`, `report`, `referral`, `return`, `broadcast`, `complete`,
    /// `refer` or `notice`.
    pub(super) kind: &'static str,
    pub(super) body: String,
}

/// What the current task chain on a desk is for.
#[derive(Clone, Debug)]
pub(super) enum Mode {
    /// Work on the hive's pipeline, reached at `hop`.
    Pipeline { hop: u32 },
    /// Answering a question another desk asked.
    Answering { origin: ReferralOrigin, hop: u32 },
}

/// Work waiting for a desk to be free.
#[derive(Clone, Debug)]
pub(super) struct Incoming {
    pub(super) mode: Mode,
    pub(super) assign: Vec<String>,
    pub(super) reason: &'static str,
    pub(super) row: Row,
}

/// Why a seat is in the round it is in.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Assignment {
    /// `opening`, `report`, `referral`, `return`, `broadcast` or `handoff`.
    pub(super) reason: String,
    /// What it was handed.
    pub(super) note: String,
}

#[derive(Debug)]
pub(super) struct Floor {
    pub(super) desk: Desk,
    pub(super) hive: BoundHive<SeatHandle>,
    pub(super) journal: Vec<Row>,
    pub(super) next_sequence: u64,
    pub(super) state: Option<DriverState>,
    pub(super) mode: Option<Mode>,
    pub(super) resume: Option<Mode>,
    pub(super) queue: VecDeque<Incoming>,
    pub(super) notes: BTreeMap<String, Assignment>,
    pub(super) seen: BTreeMap<String, usize>,
    pub(super) rounds: u32,
    pub(super) episode_rounds: u32,
    pub(super) turns: u32,
    pub(super) pending_returns: u32,
    pub(super) asks: u32,
    pub(super) completions: Vec<(String, String, Option<String>, Option<Verdict>)>,
    pub(super) opened: bool,
    pub(super) reported: bool,
    pub(super) last_candidate: Option<String>,
    pub(super) standings: BTreeMap<String, (Vec<String>, Vec<String>)>,
}

/// The seats a floor's next round runs, and what each was handed.
#[derive(Clone, Debug)]
pub(super) struct PendingSeats {
    pub(super) desk: usize,
    pub(super) seats: Vec<String>,
    pub(super) blind: bool,
    pub(super) phase: &'static str,
}

/// The hive's answer, once the last desk reports.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Final {
    pub(super) desk: String,
    pub(super) seat: String,
    pub(super) candidate: Option<String>,
    pub(super) report: String,
}

/// Every desk's floor and the links between them.
pub(super) struct Federation {
    pub(super) floors: Vec<Floor>,
    pub(super) everyone: Vec<RosterMember>,
    pub(super) desks: Vec<Desk>,
    pub(super) links: Vec<(String, String)>,
    pub(super) topology: Vec<(String, String)>,
    pub(super) policy: ReferralPolicy,
    pub(super) routing: RoutingPolicy,
    pub(super) round: u32,
    pub(super) finished: Option<Final>,
    pub(super) referrals: Vec<ReferralFrame>,
}

impl Federation {
    /// Build one floor per declared desk.
    ///
    /// # Errors
    ///
    /// Fails when core rejects a desk graph.
    pub(super) fn new(lowered: &LoweredHive) -> anyhow::Result<Self> {
        let mut floors = Vec::new();
        let mut everyone: Vec<RosterMember> = Vec::new();
        for desk in &lowered.desks {
            let candidates = lowered
                .candidates
                .iter()
                .filter(|candidate| desk.members.contains(&candidate.id))
                .cloned()
                .collect();
            let bindings = desk
                .members
                .iter()
                .map(|id| AgentBinding::new(id.clone(), SeatHandle(id.clone())))
                .collect();
            let hive = BoundHive::new(HiveGraph::new(desk.clone(), candidates), bindings)?;
            for member in &desk.members {
                if !everyone.iter().any(|known| &known.id == member) {
                    everyone.push(RosterMember {
                        id: member.clone(),
                        name: Some(member.clone()),
                    });
                }
            }
            floors.push(Floor {
                desk: desk.clone(),
                hive,
                journal: Vec::new(),
                next_sequence: 1,
                state: None,
                mode: None,
                resume: None,
                queue: VecDeque::new(),
                notes: BTreeMap::new(),
                seen: BTreeMap::new(),
                rounds: 0,
                episode_rounds: 0,
                turns: 0,
                pending_returns: 0,
                asks: 0,
                completions: Vec::new(),
                opened: false,
                reported: false,
                last_candidate: None,
                standings: BTreeMap::new(),
            });
        }
        let pairs = |links: &[tinyhivemind_lang::Link]| -> Vec<(String, String)> {
            links
                .iter()
                .map(|link| (link.from.clone(), link.to.clone()))
                .collect()
        };
        let mut links = pairs(&lowered.topology);
        for pair in pairs(&lowered.referrals) {
            if !links.contains(&pair) {
                links.push(pair);
            }
        }
        Ok(Self {
            floors,
            everyone,
            desks: lowered.desks.clone(),
            links,
            topology: pairs(&lowered.topology),
            policy: lowered.policies.referral,
            routing: lowered.policies.routing.clone(),
            round: 0,
            finished: None,
            referrals: Vec::new(),
        })
    }

    pub(super) fn index(&self, desk: &str) -> Option<usize> {
        self.floors.iter().position(|floor| floor.desk.id == desk)
    }

    /// The desks a desk may refer to.
    pub(super) fn linked_from(&self, desk: &str) -> Vec<String> {
        self.links
            .iter()
            .filter(|(from, _)| from == desk)
            .map(|(_, to)| to.clone())
            .collect()
    }

    pub(super) fn linked(&self, from: &str, to: &str) -> bool {
        self.links.iter().any(|(a, b)| a == from && b == to)
    }

    /// The host opens the hive on its first declared desk.
    pub(super) fn open(&mut self, opening: &str) {
        let Some(floor) = self.floors.first_mut() else {
            return;
        };
        let row = floor.row("host", "opening", opening);
        let assign = floor.desk.members.clone();
        floor.queue.push_back(Incoming {
            mode: Mode::Pipeline { hop: 0 },
            assign,
            reason: "opening",
            row,
        });
    }

    /// Start queued work on idle floors, then list every floor's next round.
    ///
    /// # Errors
    ///
    /// Fails when core rejects an assignment or a driver state.
    pub(super) fn ready(&mut self) -> anyhow::Result<Vec<PendingSeats>> {
        let width = self.routing.round_width;
        let mut pending = Vec::new();
        for (index, floor) in self.floors.iter_mut().enumerate() {
            if floor.state.is_none()
                && let Some(incoming) = floor.queue.pop_front()
            {
                floor.start(incoming, width)?;
            }
            let Some(state) = &floor.state else { continue };
            let driver = CompletionDriver::new(&floor.hive, width)?;
            let round = driver.pending_round(state)?;
            let seats: Vec<String> = round
                .agents()
                .iter()
                .map(|agent| agent.hive_agent_id.to_owned())
                .collect();
            if seats.is_empty() {
                continue;
            }
            let phase = match floor.mode {
                Some(Mode::Answering { .. }) => "answer",
                _ if index + 1 == self.desks.len() => "commit",
                _ => "deliberate",
            };
            pending.push(PendingSeats {
                desk: index,
                blind: floor.episode_rounds == 0 && seats.len() > 1,
                seats,
                phase,
            });
        }
        Ok(pending)
    }

    /// The journal rows `seat` has not yet seen on its desk, and how many of
    /// the journal's rows it will have seen once it reads them.
    pub(super) fn delta(&self, desk: usize, seat: &str) -> (Vec<Row>, usize, usize) {
        let floor = &self.floors[desk];
        let from = floor.seen.get(seat).copied().unwrap_or(0);
        let rows = floor.journal[from.min(floor.journal.len())..]
            .iter()
            .filter(|row| row.author != seat)
            .cloned()
            .collect();
        (rows, floor.journal.len(), floor.journal.len())
    }

    /// Why `seat` runs this round.
    pub(super) fn assignment(&self, desk: usize, seat: &str) -> Assignment {
        self.floors[desk]
            .notes
            .get(seat)
            .cloned()
            .unwrap_or_else(|| Assignment {
                reason: "assigned".into(),
                note: "Continue your desk's open work.".into(),
            })
    }

    /// Fold one floor's completed round: commit every seat's action through
    /// the driver, route referrals, and end the episode when core says so.
    /// `results` is in the round's seat order.
    ///
    /// # Errors
    ///
    /// Fails when core rejects the round or a snapshot.
    pub(super) async fn commit(
        &mut self,
        desk: usize,
        journal_at_start: usize,
        results: Vec<(String, Action)>,
    ) -> anyhow::Result<Vec<u64>> {
        self.round += 1;
        let width = self.routing.round_width;
        let mut sequences = Vec::new();
        let mut events = Vec::new();
        let mut asks = Vec::new();
        {
            let floor = &mut self.floors[desk];
            floor.rounds += 1;
            floor.episode_rounds += 1;
            floor.turns += u32::try_from(results.len()).unwrap_or(u32::MAX);
            for (seat, action) in &results {
                floor.seen.insert(seat.clone(), journal_at_start);
                floor.notes.remove(seat);
                let (kind, body, utterance) = match action {
                    Action::Broadcast { to, message } => (
                        "broadcast",
                        format!("@{to} {message}"),
                        Utterance::Broadcast {
                            message: format!("@{to} {message}"),
                        },
                    ),
                    Action::Refer { desk: to, message } => {
                        asks.push((seat.clone(), to.clone(), message.clone()));
                        (
                            "refer",
                            format!("@#{to} {message}"),
                            Utterance::CompleteEpisode {
                                message: format!("referred to #{to}: {message}"),
                            },
                        )
                    }
                    Action::Complete {
                        report,
                        candidate,
                        verdict,
                    } => {
                        floor.completions.push((
                            seat.clone(),
                            report.clone(),
                            candidate.clone(),
                            *verdict,
                        ));
                        if let Some(candidate) = candidate {
                            floor.last_candidate = Some(candidate.clone());
                            let entry = floor.standings.entry(candidate.clone()).or_default();
                            match verdict {
                                Some(Verdict::Object) => entry.1.push(seat.clone()),
                                _ => entry.0.push(seat.clone()),
                            }
                        }
                        (
                            "complete",
                            report.clone(),
                            Utterance::CompleteEpisode {
                                message: report.clone(),
                            },
                        )
                    }
                };
                let row = floor.row(seat, kind, &body);
                sequences.push(row.sequence);
                events.push(CommittedUtterance {
                    author_id: seat.clone(),
                    sequence: Sequence(row.sequence),
                    utterance,
                });
            }
            let state = floor
                .state
                .clone()
                .ok_or_else(|| anyhow::anyhow!("a round committed on an idle floor"))?;
            let driver = CompletionDriver::new(&floor.hive, width)?;
            let round = driver.pending_round(&state)?;
            let router = MentionRouter;
            let context: Vec<String> = Vec::new();
            let broadcast = events
                .iter()
                .any(|event| matches!(event.utterance, Utterance::Broadcast { .. }));
            let transition = driver
                .apply_committed_round(
                    &state,
                    &round,
                    events,
                    broadcast.then_some(BroadcastRouting {
                        primary: Some(&router),
                        reasoning: None,
                        policy: &self.routing,
                        roster_version: 1,
                        thread_context: &context,
                    }),
                )
                .await?;
            for action in &transition.actions {
                match action {
                    HostAction::RunAgents { agent_ids, .. } => {
                        let handed = results.iter().find_map(|(seat, action)| match action {
                            Action::Broadcast { message, .. } => Some((seat, message)),
                            _ => None,
                        });
                        for id in agent_ids {
                            if let Some((from, message)) = handed {
                                floor.notes.insert(
                                    id.clone(),
                                    Assignment {
                                        reason: "broadcast".into(),
                                        note: format!("@{from} handed you: {message}"),
                                    },
                                );
                            }
                        }
                    }
                    HostAction::DeliverHandoff { agent_id, handoff } => {
                        floor.notes.insert(
                            agent_id.clone(),
                            Assignment {
                                reason: "handoff".into(),
                                note: format!("@{} handed you: {}", handoff.from, handoff.body),
                            },
                        );
                    }
                    HostAction::DeliverDm { .. } => {
                        anyhow::bail!("the euler hive serves no private messages")
                    }
                }
            }
            let complete = matches!(
                completion_status(transition.state.episode()),
                CompletionStep::Complete { .. }
            );
            floor.state = Some(transition.state);
            if complete {
                floor.state = None;
            }
        }
        for (seat, to, message) in asks {
            self.ask(desk, &seat, &to, &message)?;
        }
        if self.floors[desk].state.is_none() {
            self.finish_episode(desk)?;
        }
        Ok(sequences)
    }

    /// A desk's standings: every candidate reported on it, with its backers.
    pub(super) fn standings(&self, desk: usize) -> Vec<StandingFrame> {
        let floor = &self.floors[desk];
        let committed = if floor.reported { floor.last_candidate.clone() } else { None };
        floor
            .standings
            .iter()
            .map(|(topic, (support, object))| StandingFrame {
                topic: topic.clone(),
                support: support.len(),
                object: object.len(),
                committed: committed.as_deref() == Some(topic.as_str()),
                supporters: support.clone(),
            })
            .collect()
    }
}

impl Floor {
    pub(super) fn row(&mut self, author: &str, kind: &'static str, body: &str) -> Row {
        let row = Row {
            sequence: self.next_sequence,
            author: author.into(),
            kind,
            body: body.into(),
        };
        self.next_sequence += 1;
        self.journal.push(row.clone());
        row
    }

    fn start(&mut self, incoming: Incoming, width: usize) -> anyhow::Result<()> {
        let conversation = Conversation {
            desk_id: self.desk.id.clone(),
            desk_name: self.desk.name.clone(),
            thread_root: None,
        };
        let mut episode = CompletionEpisodeState::opened(
            conversation,
            Sequence(0),
            self.desk.members.iter().map(String::as_str),
        )?;
        for participant in &mut episode.participants {
            for record in &mut participant.assignments {
                record.completed_at = Some(Sequence(0));
            }
        }
        let episode = apply_assignment(
            &episode,
            incoming.assign.iter().map(String::as_str),
            Sequence(incoming.row.sequence),
        )?;
        let driver = CompletionDriver::new(&self.hive, width)?;
        self.state = Some(driver.start(episode)?);
        for seat in &incoming.assign {
            self.notes.insert(
                seat.clone(),
                Assignment {
                    reason: incoming.reason.into(),
                    note: incoming.row.body.clone(),
                },
            );
        }
        if incoming.reason == "refused" {
            self.pending_returns = self.pending_returns.saturating_sub(1);
            if self.pending_returns == 0 {
                self.resume = None;
            }
        }
        self.mode = Some(incoming.mode);
        self.episode_rounds = 0;
        self.opened = true;
        Ok(())
    }
}
