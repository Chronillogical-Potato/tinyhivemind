//! Several desks, one floor each, and the referrals that cross between them.
//!
//! This is the bench federation's host (`crates/tinyhivemind-core/examples/
//! bench/swarm/`) on a package's own desks: one journal and one
//! `EpisodeState` per desk, `hive::step` over that desk's members only, and a
//! pass scheduler that gives every unfinished desk one move per pass — answer
//! a referral that arrived, or take the round its own episode authorized.
//! Messages cross desks through core's real `referral` fold and nowhere else:
//! the mention is read with core's mention grammar, the routing is
//! `referral(policy, input, roster, desks)`, and the answer's way home is the
//! fold's `Return`. What this host adds is what `ReferralPolicy` leaves to a
//! host: the package's declared links, and a width bound.
//!
//! - **Links.** A referral may travel only along a declared desk link
//!   (`topology` or `referrals`, directed). A return travels back along the
//!   link its forward took. Anything else is refused as `unlinked`.
//! - **Pipeline.** A link from an earlier-declared desk to a later one is a
//!   *forward* link. A desk with forward links into it opens once every desk
//!   upstream of it has ended, so it deliberates with their reports in hand.
//!   The last declared desk answers for the hive. A link back to an earlier
//!   desk carries feedback; it can only land if that desk is still open.
//! - **Reports.** When a desk converges, its committing seat refers the
//!   settled answer to each desk it links to, one referral per link, as
//!   `!propose #<answer> ^1` plus an `@#desk` mention. The row lands in the
//!   receiving desk's journal authored by the reporting seat, so core never
//!   counts it there: a member has to back it in its own turn.
//! - **Asks.** Any seat line with an `@#desk` (or an off-desk agent) mention
//!   is a referral too. The receiving desk's chosen seat answers on its own
//!   desk and the answer returns to the asking desk. A desk may make at most
//!   [`FORWARD_CAP`] asks, a constant rather than a share of its peers.
//! - **Hops.** A report carries the deepest hop that reached its desk, so a
//!   pipeline's depth is bounded by `max_hops`; an ask starts at hop zero.

use std::collections::VecDeque;

use tinyhivemind_core::dispatch::{DispatchConversation, DispatchKey};
use tinyhivemind_core::hive::{
    Conversation, EpisodeState, HiveStep, Sequence, SessionAuthor, SessionMessage, desk::DeskSet,
    roster::Roster, step,
};
use tinyhivemind_core::mention::{MentionAuthor, MentionTarget, resolve as mentions_in};
use tinyhivemind_core::referral::{
    Referral, ReferralDecision, ReferralInput, ReferralKind, ReferralOrigin, referral,
};
use tinyhivemind_core::telemetry::Tracer;

use crate::Result;
use crate::episode::{
    Ending, Event, Outcome, RoundRecord, Speaker, ending_of, fold, member_ids, message, play,
    speech,
};
use crate::hive::Hive;

/// Asks one desk may refer to other desks across its episode.
pub(crate) const FORWARD_CAP: u32 = 2;

/// One referral as the run routed it, or refused it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReferralRecord {
    /// The run's round counter when it was routed.
    pub(crate) round: u32,
    /// `report`, `forward` or `return`.
    pub(crate) kind: &'static str,
    /// The desk it left.
    pub(crate) from_desk: String,
    /// The desk it was for.
    pub(crate) to_desk: String,
    /// The seat that wrote it.
    pub(crate) seat: String,
    /// The seat core chose to receive it, when it chose one.
    pub(crate) target: Option<String>,
    /// What it carried.
    pub(crate) content: String,
    /// Its depth: the child hop for a routed one, the hop it stopped at for a
    /// refused one.
    pub(crate) hops: u32,
    /// Whether it landed.
    pub(crate) accepted: bool,
    /// Why not, when it did not: a core `NoReferralReason` in `snake_case`,
    /// or `unlinked`, `desk_closed` or `width_cap` from this host.
    pub(crate) reason: Option<String>,
    /// The sequence it landed at in the receiving desk's journal.
    pub(crate) sequence: Option<u64>,
}

/// How one desk's own episode ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeskEnd {
    /// The desk.
    pub(crate) desk: String,
    /// Its ending.
    pub(crate) ending: Ending,
    /// Rounds it took.
    pub(crate) rounds: u32,
    /// Turns its seats took, including referral answers.
    pub(crate) turns: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Sent {
    Report,
    Line,
}

#[derive(Debug)]
struct Floor {
    journal: Vec<SessionMessage>,
    state: Option<EpisodeState>,
    ended: Option<Ending>,
    rounds: u32,
    turns: u32,
    hop: u32,
    forwards: u32,
    pending: VecDeque<Referral>,
}

/// One line a referral may be read out of.
struct Line<'a> {
    author: &'a str,
    content: &'a str,
    sequence: u64,
    hop: u32,
    origin: Option<ReferralOrigin>,
    sent: Sent,
}

struct Federation<'h, 'o, 't, 's> {
    hive: &'h Hive,
    floors: Vec<Floor>,
    round: u32,
    tracer: Option<&'t Tracer<'s>>,
    observe: &'o mut dyn FnMut(Event<'_>),
}

/// Run every desk of `hive` to termination, routing referrals between them.
///
/// The hive's ending is the last declared desk's; a metered speaker that runs
/// out ends every unfinished desk (and so the hive) exhausted.
///
/// # Errors
///
/// Fails when core rejects a snapshot or policy, or the speaker fails.
pub(crate) fn run_hive(
    hive: &Hive,
    speaker: &mut dyn Speaker,
    tracer: Option<&Tracer<'_>>,
    observe: &mut dyn FnMut(Event<'_>),
) -> Result<Outcome> {
    let floors = hive
        .rooms
        .iter()
        .map(|room| Floor {
            journal: vec![message(1, SessionAuthor::Operator, &room.opening)],
            state: None,
            ended: None,
            rounds: 0,
            turns: 0,
            hop: 0,
            forwards: 0,
            pending: VecDeque::new(),
        })
        .collect();
    let mut federation = Federation {
        hive,
        floors,
        round: 0,
        tracer,
        observe,
    };
    let spent = federation.drive(speaker)?;
    let answer = &federation.floors[hive.answer()];
    let ending = match answer.ended.clone() {
        Some(Ending::Unopened) | None => Ending::Exhausted,
        Some(ending) => ending,
    };
    Ok(Outcome {
        ending,
        rounds: federation.round,
        turns: federation.floors.iter().map(|floor| floor.turns).sum(),
        spent,
    })
}

impl Federation<'_, '_, '_, '_> {
    /// Returns whether a metered speaker's budget stopped the run.
    fn drive(&mut self, speaker: &mut dyn Speaker) -> Result<bool> {
        let count = self.floors.len();
        let mut spent = false;
        loop {
            let mut progressed = false;
            for desk in 0..count {
                if self.floors[desk].ended.is_some() {
                    // Answers owed by a desk that has finished are stranded.
                    self.floors[desk].pending.clear();
                    continue;
                }
                if let Some(incoming) = self.floors[desk].pending.pop_front() {
                    if speaker.remaining() == Some(0) {
                        spent = true;
                        break;
                    }
                    self.deliver(desk, &incoming, speaker)?;
                    progressed = true;
                    continue;
                }
                if self.floors[desk].state.is_none() {
                    let ready = self
                        .hive
                        .upstream(desk)
                        .iter()
                        .all(|from| self.floors[*from].ended.is_some());
                    if !ready {
                        continue;
                    }
                    self.open(desk);
                }
                if !self.take_round(desk, speaker)? {
                    spent = true;
                    break;
                }
                progressed = true;
            }
            if spent || !progressed {
                break;
            }
        }
        for desk in 0..count {
            if self.floors[desk].ended.is_none() {
                let ending = if self.floors[desk].state.is_some() {
                    Ending::Exhausted
                } else {
                    Ending::Unopened
                };
                self.finish(desk, ending)?;
            }
        }
        Ok(spent)
    }

    fn open(&mut self, desk: usize) {
        let room = &self.hive.rooms[desk];
        let floor = &mut self.floors[desk];
        let watermark = floor
            .journal
            .last()
            .map_or(Sequence(0), |message| message.sequence);
        floor.state = Some(EpisodeState::opened(
            Conversation {
                desk_id: room.desk.id.clone(),
                desk_name: room.desk.name.clone(),
                thread_root: None,
            },
            watermark,
        ));
    }

    /// One `step` on one desk: speak its round, or end it. Returns `false`
    /// when a metered speaker cannot fill the round.
    fn take_round(&mut self, desk: usize, speaker: &mut dyn Speaker) -> Result<bool> {
        let hive = self.hive;
        let room = &hive.rooms[desk];
        let Some(state) = self.floors[desk].state.clone() else {
            return Ok(true);
        };
        let decision = {
            let roster = Roster::new(&hive.everyone, &[], &[]);
            let desk_set = DeskSet::new(&hive.desks, &[], &[], &[], &[]);
            step(
                &state,
                &self.floors[desk].journal,
                &roster,
                &desk_set,
                &room.policy,
            )?
        };
        if let Some(tracer) = self.tracer {
            tracer.step(&decision);
        }
        if let Some(ending) = ending_of(&decision) {
            self.finish(desk, ending)?;
            return Ok(true);
        }
        let HiveStep::Speak { turns, next_state } = decision else {
            return Ok(true);
        };
        if speaker
            .remaining()
            .is_some_and(|left| (left as usize) < turns.len())
        {
            return Ok(false);
        }
        self.round += 1;
        let floor = &mut self.floors[desk];
        floor.rounds += 1;
        let records = play(room, &turns, &mut floor.journal, speaker)?;
        floor.turns += u32::try_from(records.len()).unwrap_or(u32::MAX);
        floor.state = Some(*next_state);
        let (standings, committed) = fold(&floor.journal, &room.policy, &member_ids(room))?;
        let record = RoundRecord {
            round: self.round,
            desk: room.desk.id.clone(),
            desk_round: floor.rounds,
            phase: turns.first().map(|turn| turn.phase),
            visibility: turns
                .first()
                .map_or(tinyhivemind_core::hive::Visibility::Blind, |turn| {
                    turn.visibility
                }),
            turns: records,
            standings,
            committed,
        };
        (self.observe)(Event::Round(&record));
        for turn in &record.turns {
            self.route(
                desk,
                Line {
                    author: &turn.seat,
                    content: &turn.content,
                    sequence: turn.sequence,
                    hop: 0,
                    origin: None,
                    sent: Sent::Line,
                },
            )?;
        }
        Ok(true)
    }

    /// The seat a forward chose answers on its own desk; the answer goes back.
    fn deliver(
        &mut self,
        desk: usize,
        incoming: &Referral,
        speaker: &mut dyn Speaker,
    ) -> Result<()> {
        let hive = self.hive;
        let room = &hive.rooms[desk];
        let visible = self.floors[desk].journal.clone();
        let content = speaker.answer(&incoming.target_id, &incoming.content, &visible)?;
        let floor = &mut self.floors[desk];
        let sequence = next_sequence(&floor.journal);
        let of = floor.journal.len();
        floor
            .journal
            .push(speech(sequence, &incoming.target_id, &content));
        floor.turns += 1;
        self.round += 1;
        let (standings, committed) = fold(&floor.journal, &room.policy, &member_ids(room))?;
        let record = RoundRecord {
            round: self.round,
            desk: room.desk.id.clone(),
            desk_round: floor.rounds,
            phase: None,
            visibility: tinyhivemind_core::hive::Visibility::Full,
            turns: vec![crate::episode::TurnRecord {
                seat: incoming.target_id.clone(),
                desk: room.home_of(&incoming.target_id),
                reason: "referral".into(),
                saw: visible.len(),
                of,
                content: content.clone(),
                sequence,
            }],
            standings,
            committed,
        };
        (self.observe)(Event::Round(&record));
        self.route(
            desk,
            Line {
                author: &incoming.target_id,
                content: &content,
                sequence,
                hop: incoming.child_hop,
                origin: incoming.origin.clone(),
                sent: Sent::Line,
            },
        )
    }

    /// Record a desk's ending and send its settled answer down its links.
    fn finish(&mut self, desk: usize, ending: Ending) -> Result<()> {
        let floor = &mut self.floors[desk];
        floor.ended = Some(ending.clone());
        let end = DeskEnd {
            desk: self.hive.rooms[desk].desk.id.clone(),
            ending: ending.clone(),
            rounds: floor.rounds,
            turns: floor.turns,
        };
        (self.observe)(Event::DeskEnd(&end));
        let Ending::Converged(topic) = ending else {
            return Ok(());
        };
        let room = &self.hive.rooms[desk];
        let (standings, _) = fold(&self.floors[desk].journal, &room.policy, &member_ids(room))?;
        let support = standings
            .iter()
            .find(|standing| standing.topic.0 == topic)
            .map_or(0, |standing| standing.supporters.len());
        let author = committer(&self.floors[desk].journal, &topic, &member_ids(room))
            .or_else(|| room.members.first().map(|member| member.id.clone()))
            .unwrap_or_default();
        let hop = self.floors[desk].hop;
        let from = room.desk.id.clone();
        let reported: Vec<String> = self
            .hive
            .links
            .iter()
            .filter(|(a, _)| *a == from)
            .map(|(_, to)| to.clone())
            .collect();
        for to in reported {
            let content = format!(
                "!propose #{topic} ^1 {} settled on {topic} with {support} grounded supporters.\n@#{to}",
                room.desk.name
            );
            let sequence = next_sequence(&self.floors[desk].journal).saturating_sub(1);
            self.route(
                desk,
                Line {
                    author: &author,
                    content: &content,
                    sequence,
                    hop,
                    origin: None,
                    sent: Sent::Report,
                },
            )?;
        }
        Ok(())
    }

    /// Core's referral decision for one line, and the desk it was meant
    /// for; `None` when the line addresses no other desk and answers nothing.
    fn decide(&self, desk: usize, line: &Line<'_>) -> Result<Option<(ReferralDecision, String)>> {
        let hive = self.hive;
        let from = &hive.rooms[desk].desk.id;
        let roster = Roster::new(&hive.everyone, &[], &[]);
        let desk_set = DeskSet::new(&hive.desks, &[], &[], &[], &[]);
        let mentions = mentions_in(
            line.content,
            None,
            &MentionAuthor::Agent {
                id: line.author.to_owned(),
            },
            &roster,
            &desk_set,
        );
        let intended = mentions
            .iter()
            .filter(|mention| !mention.quiet)
            .find_map(|mention| match &mention.target {
                MentionTarget::Desk { id } => Some(id.clone()),
                MentionTarget::Agent { id } => hive
                    .desks
                    .iter()
                    .find(|candidate| candidate.members.contains(id))
                    .map(|home| home.id.clone())
                    .filter(|home| home != from),
                MentionTarget::Person { .. } | MentionTarget::Everyone => None,
            })
            .or_else(|| {
                line.origin
                    .as_ref()
                    .map(|origin| origin.conversation.desk_id.clone())
            });
        let Some(intended) = intended else {
            return Ok(None);
        };
        let input = ReferralInput {
            key: DispatchKey {
                trigger_sequence: line.sequence,
            },
            conversation: DispatchConversation {
                desk_id: from.clone(),
                thread_root: None,
            },
            author_id: line.author.to_owned(),
            content: line.content.to_owned(),
            mentions,
            hop: line.hop,
            origin: line.origin.clone(),
        };
        Ok(Some((
            referral(hive.referral, &input, &roster, &desk_set)?,
            intended,
        )))
    }

    /// Ask core whether a line owes one referral, check it against the
    /// package's links, and land it.
    fn route(&mut self, desk: usize, line: Line<'_>) -> Result<()> {
        let Some((decision, intended)) = self.decide(desk, &line)? else {
            return Ok(());
        };
        let Line {
            author,
            content,
            hop,
            origin,
            sent,
            ..
        } = line;
        let hive = self.hive;
        let from = hive.rooms[desk].desk.id.clone();
        let mut record = ReferralRecord {
            round: self.round,
            kind: match sent {
                Sent::Report => "report",
                Sent::Line if origin.is_some() => "return",
                Sent::Line => "forward",
            },
            from_desk: from.clone(),
            to_desk: intended,
            seat: author.to_owned(),
            target: None,
            content: content.to_owned(),
            hops: hop,
            accepted: false,
            reason: None,
            sequence: None,
        };
        let one = match decision {
            ReferralDecision::None { reason } => {
                record.reason = Some(snake(&format!("{reason:?}")));
                (self.observe)(Event::Referral(&record));
                return Ok(());
            }
            ReferralDecision::One { referral } => *referral,
        };
        // A referral that stays on this desk is already where its reader is.
        if !one.crosses() {
            return Ok(());
        }
        record.to_desk.clone_from(&one.to.desk_id);
        record.target = Some(one.target_id.clone());
        record.hops = one.child_hop;
        if sent == Sent::Line {
            record.kind = match one.kind {
                ReferralKind::Forward => "forward",
                ReferralKind::Return => "return",
            };
        }
        let linked = match one.kind {
            ReferralKind::Forward => hive.linked(&from, &one.to.desk_id),
            ReferralKind::Return => {
                hive.linked(&one.to.desk_id, &from) || hive.linked(&from, &one.to.desk_id)
            }
        };
        let target = hive.index(&one.to.desk_id);
        let refusal = match target {
            _ if !linked => Some("unlinked"),
            None => Some("unknown_desk"),
            Some(index) if self.floors[index].ended.is_some() => Some("desk_closed"),
            _ if sent == Sent::Line
                && one.kind == ReferralKind::Forward
                && self.floors[desk].forwards >= FORWARD_CAP =>
            {
                Some("width_cap")
            }
            _ => None,
        };
        let Some(index) = target.filter(|_| refusal.is_none()) else {
            record.reason = refusal.map(str::to_owned);
            (self.observe)(Event::Referral(&record));
            return Ok(());
        };
        let landed = next_sequence(&self.floors[index].journal);
        self.floors[index]
            .journal
            .push(speech(landed, author, content));
        record.accepted = true;
        record.sequence = Some(landed);
        match (sent, one.kind) {
            (Sent::Report, _) => {
                let floor = &mut self.floors[index];
                floor.hop = floor.hop.max(one.child_hop);
            }
            (Sent::Line, ReferralKind::Forward) => {
                self.floors[desk].forwards += 1;
                self.floors[index].pending.push_back(one);
            }
            (Sent::Line, ReferralKind::Return) => {}
        }
        (self.observe)(Event::Referral(&record));
        Ok(())
    }
}

/// The member who last committed `topic`, the seat that reports it.
fn committer(journal: &[SessionMessage], topic: &str, members: &[&str]) -> Option<String> {
    tinyhivemind_core::hive::trace::read(journal)
        .into_iter()
        .rev()
        .find(|trace| {
            trace.kind == tinyhivemind_core::hive::TraceKind::Commit
                && trace.topic.as_ref().is_some_and(|id| id.0 == topic)
                && trace.agent_id().is_some_and(|id| members.contains(&id))
        })
        .and_then(|trace| trace.agent_id().map(str::to_owned))
}

fn next_sequence(journal: &[SessionMessage]) -> u64 {
    u64::try_from(journal.len())
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}

/// `HopLimitReached` to `hop_limit_reached`.
pub(crate) fn snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (index, character) in name.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.extend(character.to_lowercase());
        } else {
            out.push(character);
        }
    }
    out
}
