//! Run one model-free episode of a lowered package through core's `hive::step`.
//!
//! The loop is `crates/tinyhivemind-core/examples/hive.rs`'s: fold, authorize a
//! round, let each authorized seat speak against its own projection, append,
//! repeat. What a seat says comes from a [`Speaker`]: a script, the seeded
//! default script, or the math simulation. The ending always comes from core —
//! this file never decides who won.

use std::collections::BTreeMap;

use tinyhivemind_core::hive::aside::Audience;
use tinyhivemind_core::hive::{
    Conversation, EpisodePolicy, EpisodeState, HiveStep, HiveTurn, Phase, Sequence, SessionAuthor,
    SessionMessage, TopicStanding, TraceKind, Visibility,
    desk::{Desk, DeskSet, ResponderMode},
    project_for,
    quorum::standings,
    roster::{Roster, RosterMember},
    step,
    trace::read,
};
use tinyhivemind_core::telemetry::Tracer;
use tinyhivemind_lang::LoweredHive;

use crate::Result;

/// The desk id an episode runs on when a package declares more than one desk.
pub(crate) const FLOOR: &str = "floor";

/// Everything one episode runs over.
#[derive(Clone, Debug)]
pub(crate) struct Room {
    /// Package id, for reports.
    pub(crate) package: String,
    /// Every seat, as roster members.
    pub(crate) members: Vec<RosterMember>,
    /// The one desk the episode runs on.
    pub(crate) desk: Desk,
    /// Each seat's home desk (its first declared desk).
    pub(crate) home: BTreeMap<String, String>,
    /// The episode policy, after knobs.
    pub(crate) policy: EpisodePolicy,
    /// The operator message that opens the episode.
    pub(crate) opening: String,
}

impl Room {
    /// The room for a lowered package.
    ///
    /// A one-desk package runs on that desk. A package with several desks runs
    /// one episode on the *hive floor*: every seat of every desk, in declared
    /// desk order, so quorum is hive-wide. Topology and referrals are reported
    /// by the renderers but not routed by this offline loop.
    pub(crate) fn of(lowered: &LoweredHive, opening: String) -> Self {
        let desk = match lowered.desks.as_slice() {
            [only] => only.clone(),
            desks => {
                let mut members: Vec<String> = Vec::new();
                for desk in desks {
                    for member in &desk.members {
                        if !members.contains(member) {
                            members.push(member.clone());
                        }
                    }
                }
                Desk {
                    id: FLOOR.into(),
                    name: "Hive floor".into(),
                    description: Some(format!("Every seat of {}", lowered.name)),
                    members,
                    responder_mode: ResponderMode::Auto,
                }
            }
        };
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
            desk,
            home,
            policy: lowered.policies.episode,
            opening,
        }
    }
}

/// What a seat says when it is authorized to speak.
pub(crate) trait Speaker {
    /// The content of one authorized turn, given what that turn may see.
    fn speak(&mut self, turn: &HiveTurn, visible: &[SessionMessage], policy: &EpisodePolicy)
    -> String;
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
    /// One-based round number.
    pub(crate) round: u32,
    /// The round's phase.
    pub(crate) phase: Phase,
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
}

impl Ending {
    /// The `snake_case` name the frames use.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Converged(_) => "converged",
            Self::Deadlocked(_) => "deadlocked",
            Self::Exhausted => "exhausted",
            Self::Idle => "idle",
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
    /// How it ended.
    pub(crate) ending: Ending,
    /// Speaking rounds taken.
    pub(crate) rounds: u32,
    /// Turns taken.
    pub(crate) turns: u32,
}

/// Drive one episode to termination.
///
/// `observe` is called once per round, after the round is appended.
///
/// # Errors
///
/// Fails when core rejects the roster, desks or policy.
pub(crate) fn run(
    room: &Room,
    speaker: &mut dyn Speaker,
    tracer: Option<&Tracer<'_>>,
    observe: &mut dyn FnMut(&RoundRecord),
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
        let (turns, next_state) = match decision {
            HiveStep::Speak { turns, next_state } => (turns, next_state),
            HiveStep::Converged { topic, .. } => return Ok(outcome(Ending::Converged(topic.0), rounds, turns_taken)),
            HiveStep::Deadlocked { topics } => {
                let topics = topics.into_iter().map(|topic| topic.0).collect();
                return Ok(outcome(Ending::Deadlocked(topics), rounds, turns_taken));
            }
            HiveStep::Exhausted { .. } => return Ok(outcome(Ending::Exhausted, rounds, turns_taken)),
            HiveStep::Idle => return Ok(outcome(Ending::Idle, rounds, turns_taken)),
        };
        rounds += 1;
        let mut records = Vec::with_capacity(turns.len());
        // Every turn in a round speaks against the journal as it stood when
        // the round was authorized; `project_for` withholds concurrent rows.
        for turn in &turns {
            let visible = project_for(turn, &journal);
            let content = speaker.speak(turn, &visible, &room.policy);
            let sequence = u64::try_from(journal.len()).unwrap_or(u64::MAX).saturating_add(1);
            records.push(TurnRecord {
                seat: turn.agent_id.clone(),
                desk: room.home.get(&turn.agent_id).cloned().unwrap_or_else(|| room.desk.id.clone()),
                reason: format!("{:?}", turn.reason).to_lowercase(),
                saw: visible.len(),
                of: journal.len(),
                content: content.clone(),
                sequence,
            });
            journal.push(speech(sequence, &turn.agent_id, &content));
        }
        turns_taken += u32::try_from(turns.len()).unwrap_or(u32::MAX);
        let (standings, committed) = fold(&journal, &room.policy)?;
        observe(&RoundRecord {
            round: rounds,
            phase: turns.first().map_or(Phase::Deliberate, |turn| turn.phase),
            visibility: turns.first().map_or(Visibility::Blind, |turn| turn.visibility),
            turns: records,
            standings,
            committed,
        });
        state = *next_state;
    }
}

fn outcome(ending: Ending, rounds: u32, turns: u32) -> Outcome {
    Outcome {
        ending,
        rounds,
        turns,
    }
}

/// Standings and committed topics over everything agents wrote.
///
/// The same fold `step` makes: core's `trace::read` and `quorum::standings`
/// over the transcript, at its last sequence.
///
/// # Errors
///
/// Fails on a malformed quorum policy.
pub(crate) fn fold(
    journal: &[SessionMessage],
    policy: &EpisodePolicy,
) -> Result<(Vec<TopicStanding>, Vec<String>)> {
    let traces = read(journal);
    let at = journal.last().map_or(Sequence(0), |message| message.sequence);
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
