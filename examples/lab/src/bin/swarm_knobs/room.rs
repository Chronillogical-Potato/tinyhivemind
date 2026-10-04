//! A scripted room: five seats whose lines are a pure function of what the
//! algebra lets them see, driven by `hive::step` until it stops speaking.

use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::hive::quorum::{ConsensusState, consensus, standings};
use tinyhivemind_core::hive::trace::{TraceKind, read};
use tinyhivemind_core::hive::{
    AgentThreshold, EpisodePolicy, EpisodeState, HiveStep, HiveTurn, Phase, project_for, step,
};
use tinyhivemind_core::runtime::{Conversation, Sequence, SessionAuthor, SessionMessage};
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};
use tinyhivemind_lab::World;

/// A seat and the plan it argues for.
#[derive(Clone, Copy)]
pub struct Seat {
    pub id: &'static str,
    pub favourite: &'static str,
}

/// The cast: two seats for `x`, two for `y`, one undecided-leaning `z`.
pub const CAST: [Seat; 5] = [
    Seat {
        id: "ada",
        favourite: "x",
    },
    Seat {
        id: "ben",
        favourite: "x",
    },
    Seat {
        id: "cy",
        favourite: "y",
    },
    Seat {
        id: "di",
        favourite: "y",
    },
    Seat {
        id: "eli",
        favourite: "z",
    },
];

/// How the room is run, apart from the episode policy under test.
#[derive(Clone, Default)]
pub struct Room {
    /// Seats write supports and proposals with no citation.
    pub sloppy: bool,
    /// Private rows the host appends after every public one. The fold ignores
    /// them; the sequence numbering does not.
    pub chatter: usize,
    /// Thresholds the episode opens with.
    pub thresholds: Vec<AgentThreshold>,
}

/// What a run came to.
#[derive(Debug)]
pub struct Outcome {
    /// Calls to `step` that authorized speakers.
    pub rounds: usize,
    /// Turns authorized.
    pub turns: u32,
    /// The most turns one round authorized.
    pub widest: usize,
    /// How the episode ended.
    pub end: String,
    /// Who spoke first, in order.
    pub order: Vec<&'static str>,
}

pub fn world() -> World {
    let ids: Vec<&str> = CAST.iter().map(|seat| seat.id).collect();
    CAST.iter()
        .fold(World::new(), |world, seat| world.agent(seat.id))
        .desk("war", "War Room", "Pick a plan", &ids)
}

fn msg(sequence: u64, author: SessionAuthor, content: &str, audience: Audience) -> SessionMessage {
    SessionMessage {
        sequence: Sequence(sequence),
        author,
        content: content.into(),
        audience,
        elided: None,
    }
}

fn seat_author(id: &str) -> SessionAuthor {
    SessionAuthor::Agent {
        id: id.into(),
        label: id.into(),
    }
}

fn says(
    seat: &Seat,
    turn: &HiveTurn,
    visible: &[SessionMessage],
    policy: &EpisodePolicy,
    sloppy: bool,
) -> String {
    let cite = |at: u64| {
        if sloppy {
            String::new()
        } else {
            format!(" ^{at}")
        }
    };
    let traces = read(visible);
    let at = visible.last().map_or(Sequence(0), |row| row.sequence);
    let table = standings(&traces, at, &policy.quorum).unwrap_or_default();
    if turn.phase == Phase::Commit {
        return match consensus(&table, &policy.quorum) {
            ConsensusState::Quorum { topic } => format!("!commit #{topic} recorded"),
            _ => "!question what are we recording?".into(),
        };
    }
    let mine = traces
        .iter()
        .any(|t| t.agent_id() == Some(seat.id) && t.kind == TraceKind::Propose);
    if !mine {
        return format!("!propose #{} my plan{}", seat.favourite, cite(1));
    }
    let proposal = |topic: &str| {
        traces
            .iter()
            .find(|t| {
                t.kind == TraceKind::Propose
                    && t.topic.as_ref().is_some_and(|x| x.as_str() == topic)
            })
            .map(|t| t.sequence.0)
    };
    if turn.reason == tinyhivemind_core::hive::BidReason::Dissent {
        let other = traces.iter().find(|t| {
            t.kind == TraceKind::Propose
                && t.agent_id() != Some(seat.id)
                && t.topic
                    .as_ref()
                    .is_some_and(|x| x.as_str() != seat.favourite)
        });
        if let Some(other) = other {
            return format!(
                "!object >{} ^{} that plan is weaker",
                other.sequence.0, other.sequence.0
            );
        }
    }
    let leader = table
        .iter()
        .max_by_key(|s| (s.supporters.len(), std::cmp::Reverse(s.topic.clone())))
        .map(|s| s.topic.as_str().to_owned())
        .unwrap_or_else(|| seat.favourite.to_owned());
    match proposal(&leader) {
        Some(seq) => format!("!support #{leader} agree{}", cite(seq)),
        None => format!("!evidence restating{}", cite(1)),
    }
}

/// Drive one episode to its end, tracing every step.
pub fn run(room: &Room, policy: &EpisodePolicy, tracer: &Tracer<'_>) -> Result<Outcome, String> {
    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let conversation = Conversation {
        desk_id: "war".into(),
        desk_name: "War Room".into(),
        thread_root: None,
    };
    let mut state = EpisodeState::opened(conversation, Sequence(1));
    state.thresholds = room.thresholds.clone();
    let mut journal = vec![msg(
        1,
        SessionAuthor::Operator,
        "Pick a plan.",
        Audience::Desk,
    )];
    let mut outcome = Outcome {
        rounds: 0,
        turns: 0,
        widest: 0,
        end: String::new(),
        order: Vec::new(),
    };
    for _ in 0..80 {
        let decided = step(&state, &journal, &roster, &desks, policy).map_err(|e| e.to_string())?;
        tracer.step(&decided);
        let HiveStep::Speak { turns, next_state } = decided else {
            outcome.end = match &decided {
                HiveStep::Converged { topic, .. } => format!("converged #{topic}"),
                HiveStep::Deadlocked { topics } => format!("deadlocked {}", topics.len()),
                HiveStep::Exhausted { spent, .. } => format!("exhausted at {spent}"),
                HiveStep::Idle => "idle".into(),
                HiveStep::Speak { .. } => unreachable!("a speaking step was matched above"),
            };
            return Ok(outcome);
        };
        outcome.rounds += 1;
        outcome.widest = outcome.widest.max(turns.len());
        let lines: Vec<(&Seat, String)> = turns
            .iter()
            .filter_map(|turn| {
                let seat = CAST.iter().find(|seat| seat.id == turn.agent_id)?;
                let visible = project_for(turn, &journal);
                let line = says(seat, turn, &visible, policy, room.sloppy);
                Some((seat, line))
            })
            .collect();
        for (seat, line) in lines {
            outcome.turns += 1;
            if outcome.order.len() < 6 {
                outcome.order.push(seat.id);
            }
            let next = journal.len() as u64 + 1;
            journal.push(msg(next, seat_author(seat.id), &line, Audience::Desk));
            for _ in 0..room.chatter {
                let next = journal.len() as u64 + 1;
                let to = Audience::Aside {
                    members: vec!["ben".into()],
                };
                journal.push(msg(next, seat_author("ada"), "side remark", to));
            }
        }
        state = *next_state;
    }
    tracer.emit(TraceEvent::Checkpoint {
        label: "step cap reached".into(),
    });
    outcome.end = "step cap".into();
    Ok(outcome)
}
