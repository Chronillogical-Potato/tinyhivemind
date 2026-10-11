//! Run one episode of one desk through core's `hive::step`.
//!
//! The loop is `crates/tinyhivemind-core/examples/hive.rs`'s: fold, authorize a
//! round, let each authorized seat speak against its own projection, append,
//! repeat. What a seat says comes from a [`Speaker`]: a script, the seeded
//! default script, the math simulation, or (`--live`) a model. The ending
//! always comes from core — this file never decides who won. A package of
//! several desks runs one of these floors per desk; `desks.rs` schedules them
//! and routes referrals between them.

use std::collections::BTreeMap;

use tinyhivemind_core::hive::aside::Audience;
use tinyhivemind_core::hive::{
    Conversation, EpisodePolicy, EpisodeState, HiveStep, HiveTurn, Phase, Sequence, SessionAuthor,
    SessionMessage, TopicStanding, TraceKind, Visibility,
    desk::{Desk, DeskSet},
    project_for,
    quorum::standings,
    roster::{Roster, RosterMember},
    step,
    trace::read,
};
use tinyhivemind_core::telemetry::Tracer;
use tinyhivemind_lang::LoweredHive;

use crate::Result;

/// Everything one desk's episode runs over.
#[derive(Clone, Debug)]
pub(crate) struct Room {
    /// Package id, for reports.
    pub(crate) package: String,
    /// The desk's seats, as roster members.
    pub(crate) members: Vec<RosterMember>,
    /// The desk the episode runs on.
    pub(crate) desk: Desk,
    /// Each seat's home desk (its first declared desk).
    pub(crate) home: BTreeMap<String, String>,
    /// The episode policy, after knobs.
    pub(crate) policy: EpisodePolicy,
    /// The operator message that opens the episode.
    pub(crate) opening: String,
}

impl Room {
    /// The room for one declared desk of a lowered package, under `policy`.
    pub(crate) fn of(
        lowered: &LoweredHive,
        desk: &Desk,
        policy: EpisodePolicy,
        opening: String,
    ) -> Self {
        let mut home = BTreeMap::new();
        for declared in &lowered.desks {
            for member in &declared.members {
                home.entry(member.clone())
                    .or_insert_with(|| declared.id.clone());
            }
        }
        Self {
            package: lowered.id.clone(),
            members: desk
                .members
                .iter()
                .map(|id| RosterMember {
                    id: id.clone(),
                    name: Some(id.clone()),
                })
                .collect(),
            desk: desk.clone(),
            home,
            policy,
            opening,
        }
    }

    /// The desk a seat's turn is reported under: its home desk, or this one.
    pub(crate) fn home_of(&self, seat: &str) -> String {
        self.home
            .get(seat)
            .cloned()
            .unwrap_or_else(|| self.desk.id.clone())
    }
}

/// What a seat says when it is authorized to speak.
pub(crate) trait Speaker {
    /// The content of one authorized turn, given what that turn may see.
    fn speak(
        &mut self,
        turn: &HiveTurn,
        visible: &[SessionMessage],
        policy: &EpisodePolicy,
    ) -> String;

    /// Every turn of one authorized round, each against its own projection.
    ///
    /// Sequential by default; a live speaker runs the round's model calls
    /// concurrently, which core allows because the projections already
    /// withhold every row written beside a turn.
    ///
    /// # Errors
    ///
    /// A live speaker fails when a call it cannot do without fails.
    fn speak_round(
        &mut self,
        round: &[(HiveTurn, Vec<SessionMessage>)],
        policy: &EpisodePolicy,
    ) -> Result<Vec<String>> {
        Ok(round
            .iter()
            .map(|(turn, visible)| self.speak(turn, visible, policy))
            .collect())
    }

    /// What `seat` writes on its desk when a referral from another desk
    /// arrives for it. The simulation never asks, so by default it says
    /// nothing further.
    ///
    /// # Errors
    ///
    /// As [`Speaker::speak_round`].
    fn answer(&mut self, seat: &str, incoming: &str, visible: &[SessionMessage]) -> Result<String> {
        let _ = (seat, incoming, visible);
        Ok(crate::script::QUIET.to_owned())
    }

    /// Turns this speaker may still fill; `None` when it is not metered.
    fn remaining(&self) -> Option<u32> {
        None
    }

    /// This speaker as `Any`, for a report that needs its concrete type.
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
}

/// One turn as it happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TurnRecord {
    /// The seat.
    pub(crate) seat: String,
    /// The seat's home desk.
    pub(crate) desk: String,
    /// Why core gave it the floor.
    pub(crate) reason: String,
    /// Rows of the transcript the turn could see.
    pub(crate) saw: usize,
    /// Rows in the transcript when the turn was taken.
    pub(crate) of: usize,
    /// What it said.
    pub(crate) content: String,
    /// The sequence its message was appended at.
    pub(crate) sequence: u64,
}

/// One round as it happened, plus the standings once it was appended.
#[derive(Clone, Debug)]
pub(crate) struct RoundRecord {
    /// One-based round number, counted across every desk of the run.
    pub(crate) round: u32,
    /// The desk the round ran on.
    pub(crate) desk: String,
    /// One-based round number on that desk.
    pub(crate) desk_round: u32,
    /// The round's phase; `None` for a seat answering a referral.
    pub(crate) phase: Option<Phase>,
    /// The round's visibility.
    pub(crate) visibility: Visibility,
    /// Its turns, in desk order.
    pub(crate) turns: Vec<TurnRecord>,
    /// Standings over the transcript after the round.
    pub(crate) standings: Vec<TopicStanding>,
    /// Topics with a `!commit` trace after the round.
    pub(crate) committed: Vec<String>,
}

/// How an episode ended, as core reported it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ending {
    /// One topic carried and was recorded.
    Converged(String),
    /// Two or more topics carried and nobody could break the tie.
    Deadlocked(Vec<String>),
    /// The turn budget ran out.
    Exhausted,
    /// Nobody bid for the floor.
    Idle,
    /// The desk never opened: the run stopped before its upstream desks ended.
    Unopened,
}

impl Ending {
    /// The `snake_case` name the frames use.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Converged(_) => "converged",
            Self::Deadlocked(_) => "deadlocked",
            Self::Exhausted => "exhausted",
            Self::Idle => "idle",
            Self::Unopened => "unopened",
        }
    }

    /// The decided topic, when there is one.
    pub(crate) fn topic(&self) -> Option<&str> {
        match self {
            Self::Converged(topic) => Some(topic),
            _ => None,
        }
    }
}

/// A finished episode.
#[derive(Clone, Debug)]
pub(crate) struct Outcome {
    /// How it ended (for several desks: how the answering desk ended).
    pub(crate) ending: Ending,
    /// Speaking rounds taken, across every desk.
    pub(crate) rounds: u32,
    /// Turns taken, across every desk, including referral answers.
    pub(crate) turns: u32,
    /// Whether a metered speaker could not fill the next round (`--max-calls`).
    pub(crate) spent: bool,
}

/// Something a run reports as it happens.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event<'a> {
    /// A round was appended.
    Round(&'a RoundRecord),
    /// A referral was routed, or refused.
    Referral(&'a crate::desks::ReferralRecord),
    /// One desk's episode ended.
    DeskEnd(&'a crate::desks::DeskEnd),
}

/// Speak one authorized round against `journal` and append it.
///
/// Every turn speaks against the journal as it stood when the round was
/// authorized: `project_for` withholds each row a peer writes beside it, so
/// computing every projection first is the same as computing each one as
/// its turn comes up. `of` and `sequence` are what they would be appended one
/// at a time.
///
/// # Errors
///
/// As [`Speaker::speak_round`], or when it returns the wrong number of lines.
pub(crate) fn play(
    room: &Room,
    turns: &[HiveTurn],
    journal: &mut Vec<SessionMessage>,
    speaker: &mut dyn Speaker,
) -> Result<Vec<TurnRecord>> {
    let base = journal.len();
    let views: Vec<(HiveTurn, Vec<SessionMessage>)> = turns
        .iter()
        .map(|turn| (turn.clone(), project_for(turn, journal)))
        .collect();
    let said = speaker.speak_round(&views, &room.policy)?;
    if said.len() != views.len() {
        return Err("a speaker answered a round with the wrong number of lines".into());
    }
    let mut records = Vec::with_capacity(views.len());
    for (index, ((turn, visible), content)) in views.iter().zip(said).enumerate() {
        let sequence = u64::try_from(base + index + 1).unwrap_or(u64::MAX);
        records.push(TurnRecord {
            seat: turn.agent_id.clone(),
            desk: room.home_of(&turn.agent_id),
            reason: format!("{:?}", turn.reason).to_lowercase(),
            saw: visible.len(),
            of: base + index,
            content: content.clone(),
            sequence,
        });
        journal.push(speech(sequence, &turn.agent_id, &content));
    }
    Ok(records)
}

/// The ending a terminal step reports; `None` for a round to speak.
pub(crate) fn ending_of(decision: &HiveStep) -> Option<Ending> {
    match decision {
        HiveStep::Speak { .. } => None,
        HiveStep::Converged { topic, .. } => Some(Ending::Converged(topic.0.clone())),
        HiveStep::Deadlocked { topics } => Some(Ending::Deadlocked(
            topics.iter().map(|topic| topic.0.clone()).collect(),
        )),
        HiveStep::Exhausted { .. } => Some(Ending::Exhausted),
        HiveStep::Idle => Some(Ending::Idle),
    }
}

/// The member ids of a room, for folds that count only members.
pub(crate) fn member_ids(room: &Room) -> Vec<&str> {
    room.members
        .iter()
        .map(|member| member.id.as_str())
        .collect()
}

/// Drive one desk's episode to termination.
///
/// `observe` is called once per round, after the round is appended. A
/// metered speaker that cannot fill the next round ends the episode
/// exhausted.
///
/// # Errors
///
/// Fails when core rejects the roster, desks or policy, or a speaker fails.
pub(crate) fn run(
    room: &Room,
    speaker: &mut dyn Speaker,
    tracer: Option<&Tracer<'_>>,
    observe: &mut dyn FnMut(Event<'_>),
) -> Result<Outcome> {
    let mut journal = vec![message(1, SessionAuthor::Operator, &room.opening)];
    let conversation = Conversation {
        desk_id: room.desk.id.clone(),
        desk_name: room.desk.name.clone(),
        thread_root: None,
    };
    let mut state = EpisodeState::opened(conversation, Sequence(1));
    let desks = [room.desk.clone()];
    let (mut rounds, mut turns_taken) = (0_u32, 0_u32);
    loop {
        let roster = Roster::new(&room.members, &[], &[]);
        let desk_set = DeskSet::new(&desks, &[], &[], &[], &[]);
        let decision = step(&state, &journal, &roster, &desk_set, &room.policy)?;
        if let Some(tracer) = tracer {
            tracer.step(&decision);
        }
        if let Some(ending) = ending_of(&decision) {
            return Ok(outcome(ending, rounds, turns_taken));
        }
        let HiveStep::Speak { turns, next_state } = decision else {
            return Ok(outcome(Ending::Idle, rounds, turns_taken));
        };
        if speaker
            .remaining()
            .is_some_and(|left| (left as usize) < turns.len())
        {
            return Ok(Outcome {
                spent: true,
                ..outcome(Ending::Exhausted, rounds, turns_taken)
            });
        }
        rounds += 1;
        let records = play(room, &turns, &mut journal, speaker)?;
        turns_taken += u32::try_from(turns.len()).unwrap_or(u32::MAX);
        let (standings, committed) = fold(&journal, &room.policy, &member_ids(room))?;
        observe(Event::Round(&RoundRecord {
            round: rounds,
            desk: room.desk.id.clone(),
            desk_round: rounds,
            phase: turns.first().map(|turn| turn.phase),
            visibility: turns
                .first()
                .map_or(Visibility::Blind, |turn| turn.visibility),
            turns: records,
            standings,
            committed,
        }));
        state = *next_state;
    }
}

fn outcome(ending: Ending, rounds: u32, turns: u32) -> Outcome {
    Outcome {
        ending,
        rounds,
        turns,
        spent: false,
    }
}

/// Standings and committed topics over what the desk's members wrote.
///
/// The same fold `step` makes: core's `trace::read` and `quorum::standings`
/// over the transcript, at its last sequence, counting only traces by
/// `members` — a referral row from another desk informs, it never votes.
///
/// # Errors
///
/// Fails on a malformed quorum policy.
pub(crate) fn fold(
    journal: &[SessionMessage],
    policy: &EpisodePolicy,
    members: &[&str],
) -> Result<(Vec<TopicStanding>, Vec<String>)> {
    let traces: Vec<_> = read(journal)
        .into_iter()
        .filter(|trace| match &trace.author {
            SessionAuthor::Agent { id, .. } => members.contains(&id.as_str()),
            _ => true,
        })
        .collect();
    let at = journal
        .last()
        .map_or(Sequence(0), |message| message.sequence);
    let standing = standings(&traces, at, &policy.quorum)?;
    let mut committed: Vec<String> = Vec::new();
    for trace in &traces {
        if trace.kind == TraceKind::Commit
            && let Some(topic) = &trace.topic
            && !committed.contains(&topic.0)
        {
            committed.push(topic.0.clone());
        }
    }
    Ok((standing, committed))
}

/// A message from anyone.
pub(crate) fn message(sequence: u64, author: SessionAuthor, content: &str) -> SessionMessage {
    SessionMessage {
        sequence: Sequence(sequence),
        author,
        content: content.to_owned(),
        audience: Audience::Desk,
        elided: None,
    }
}

/// A message from a seat.
pub(crate) fn speech(sequence: u64, id: &str, content: &str) -> SessionMessage {
    message(
        sequence,
        SessionAuthor::Agent {
            id: id.to_owned(),
            label: id.to_owned(),
        },
        content,
    )
}
