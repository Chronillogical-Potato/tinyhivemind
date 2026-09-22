//! Shared fixtures: a hive of named seats, a router that always asks for
//! clarification, a journal, and one wave driven the way a host drives it.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
use std::sync::Mutex;

use tinyhivemind::desk::{Desk, ResponderMode};
use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind::{Sequence, responder::Probability};
use tinyhivemind_embed::{
    CandidateProbability, ContributionProbability, EvaluationDisposition, RouteCandidate, Router,
    RouterFuture, RoutingEvaluation, RoutingPolicy, RoutingRequest,
};

use crate::conduct::{Commit, ConductPolicy, Conductor, Door, Event, Step, Turn};
use crate::driver::BroadcastRouting;
use crate::test_support::Seat;
use crate::{AgentBinding, BoundHive, CompletionDriver, Error, HiveGraph};

pub(super) fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(future)
}

pub(super) fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("bounded")
}

pub(super) fn policy(round_width: usize) -> RoutingPolicy {
    RoutingPolicy {
        minimum_confidence: probability(350_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(850_000),
        high_impact_threshold: probability(700_000),
        round_width,
        choice_option_limit: 8,
    }
}

pub(super) fn hive(ids: &[&str]) -> BoundHive<Seat> {
    BoundHive::new(
        HiveGraph::new(
            Desk {
                id: "engineering".into(),
                name: "Engineering".into(),
                description: None,
                members: ids.iter().map(|id| (*id).into()).collect(),
                responder_mode: ResponderMode::Auto,
            },
            ids.iter()
                .map(|id| RouteCandidate {
                    id: (*id).into(),
                    label: (*id).into(),
                    role: None,
                    description: None,
                    capabilities: Vec::new(),
                    learned_topics: Vec::new(),
                    available: true,
                })
                .collect(),
        ),
        ids.iter()
            .map(|id| AgentBinding::new(*id, Seat((*id).to_owned())))
            .collect(),
    )
    .expect("hive")
}

/// An evaluation that asks for clarification on every pass, so a broadcast
/// is placed with nobody.
#[derive(Debug)]
pub(super) struct ClarifyRouter;

impl Router for ClarifyRouter {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        let eligible: Vec<String> = request
            .candidates
            .iter()
            .map(|candidate| candidate.id.clone())
            .collect();
        let roster_version = request.roster_version;
        Box::pin(async move {
            if eligible.is_empty() {
                return Err("no candidates to route among".into());
            }
            let share = 1_000_000 / (u32::try_from(eligible.len()).expect("small") + 1);
            let mut primary_probabilities: Vec<CandidateProbability> = eligible
                .iter()
                .map(|id| CandidateProbability {
                    candidate_id: id.clone(),
                    probability: probability(share),
                })
                .collect();
            primary_probabilities.push(CandidateProbability {
                candidate_id: "none".into(),
                probability: probability(
                    1_000_000 - share * u32::try_from(eligible.len()).expect("small"),
                ),
            });
            Ok(RoutingEvaluation {
                primary_responder: eligible[0].clone(),
                primary_probabilities,
                confidence: probability(900_000),
                needs_collaboration: probability(0),
                needs_clarification: probability(1_000_000),
                contributions: eligible
                    .iter()
                    .map(|id| ContributionProbability {
                        candidate_id: id.clone(),
                        probability: probability(500_000),
                    })
                    .collect(),
                high_impact: probability(0),
                model_identity: "test".into(),
                question_schema_version: 1,
                roster_version,
                disposition: EvaluationDisposition::Unchecked,
            })
        })
    }
}

/// One row: sequence, author, body, thread, and the one seat it is for.
pub(super) type Row = (Sequence, String, String, Option<Sequence>, Option<String>);

/// The host: rows, and nothing else.
#[derive(Debug)]
pub(super) struct Journal {
    /// The sequence the first row is given.
    first: u64,
    rows: Mutex<Vec<Row>>,
}

impl Default for Journal {
    fn default() -> Self {
        Self::numbered_from(1)
    }
}

impl Journal {
    /// A journal whose first row is given `first`: some hosts number from
    /// zero.
    pub(super) fn numbered_from(first: u64) -> Self {
        Self {
            first,
            rows: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn append(
        &self,
        author: &str,
        body: &str,
        thread: Option<Sequence>,
        only_for: Option<String>,
    ) -> Sequence {
        let mut rows = self.rows.lock().unwrap();
        let sequence = Sequence(rows.last().map_or(self.first, |row| row.0.0 + 1));
        rows.push((sequence, author.into(), body.into(), thread, only_for));
        sequence
    }

    pub(super) fn latest(&self) -> Option<Sequence> {
        self.rows.lock().unwrap().last().map(|row| row.0)
    }

    pub(super) fn thread(&self, root: Sequence) -> Vec<String> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.0 == root || row.3 == Some(root))
            .map(|row| format!("@{}: {}", row.1, row.2))
            .collect()
    }

    pub(super) fn bodies(&self) -> Vec<String> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .map(|row| row.2.clone())
            .collect()
    }

    pub(super) fn private_to(&self, seat: &str) -> Vec<String> {
        self.rows
            .lock()
            .unwrap()
            .iter()
            .filter(|row| row.4.as_deref() == Some(seat))
            .map(|row| row.2.clone())
            .collect()
    }
}

/// What one wave produced, as the host saw it.
#[derive(Debug, Default)]
pub(super) struct Wave {
    pub(super) turns: Vec<Turn>,
    pub(super) events: Vec<Event>,
    /// Every commit the wave handed out, with the sequence it was given.
    pub(super) commits: Vec<(Sequence, Commit)>,
}

/// One wave: nudges, turns, the scripted calls each seat makes, and every
/// step after, appended to the journal.
pub(super) fn wave(
    conductor: &mut Conductor<'_, Seat>,
    journal: &Journal,
    calls: &[(&str, Vec<Utterance>)],
) -> Result<Wave, Error> {
    wave_parking(conductor, journal, calls, &[])
}

/// A wave in which the seats in `parked` stop on the host instead of
/// recording anything.
pub(super) fn wave_parking(
    conductor: &mut Conductor<'_, Seat>,
    journal: &Journal,
    calls: &[(&str, Vec<Utterance>)],
    parked: &[&str],
) -> Result<Wave, Error> {
    // A parked seat's calls, if it made any, are recorded before it is held.
    let mut seen = Wave::default();
    for step in conductor.begin_wave() {
        take(step, journal, &mut seen);
    }
    let turns = conductor.turns()?;
    for turn in &turns {
        let brief = conductor.open_turn(turn, journal.latest(), Vec::new(), |root| {
            journal.thread(root)
        });
        assert_eq!(brief.seat, turn.seat);
        let script = calls
            .iter()
            .find(|(seat, _)| *seat == turn.seat)
            .map(|(_, calls)| calls.clone())
            .unwrap_or_default();
        let said = script.into_iter().map(ToolCall::Speak);
        if parked.contains(&turn.seat.as_str()) {
            conductor.record_parked(turn, said);
        } else {
            conductor.record(turn, said);
        }
    }
    seen.turns = turns;
    while let Some(step) = conductor.step()? {
        if let Step::Commit(commit) = &step {
            let sequence = journal.append(
                &commit.author,
                &describe(&commit.utterance),
                commit.thread,
                commit.only_for.clone(),
            );
            run(conductor.committed(sequence))?;
            seen.commits.push((sequence, commit.clone()));
            continue;
        }
        take(step, journal, &mut seen);
    }
    Ok(seen)
}

pub(super) fn take(step: Step, journal: &Journal, seen: &mut Wave) {
    match step {
        Step::Note(note) => {
            journal.append("desk", &note.body, note.thread, note.only_for);
        }
        Step::Event(event) => seen.events.push(event),
        Step::Commit(_) => panic!("a commit is not taken, it is committed"),
    }
}

pub(super) fn describe(utterance: &Utterance) -> String {
    match utterance {
        Utterance::Post { message } | Utterance::Dm { message, .. } => message.clone(),
        Utterance::Broadcast { message } => format!("BROADCAST: {message}"),
        Utterance::Ask { to, message } => format!("asks @{to}: {message}"),
        Utterance::CompleteEpisode { message } => format!("COMPLETE: {message}"),
    }
}

pub(super) fn complete(message: &str) -> Utterance {
    Utterance::CompleteEpisode {
        message: message.into(),
    }
}

pub(super) fn ask(to: &str, message: &str) -> Utterance {
    Utterance::Ask {
        to: to.into(),
        message: message.into(),
    }
}

pub(super) fn broadcast(message: &str) -> Utterance {
    Utterance::Broadcast {
        message: message.into(),
    }
}

pub(super) fn post(message: &str) -> Utterance {
    Utterance::Post {
        message: message.into(),
    }
}

pub(super) fn door(ids: &[&str], starters: &[&str], journal: &Journal) -> Door {
    let opened_at = journal.append("operator", "the task", None, None);
    Door {
        chat: "engineering".into(),
        desk_name: "Engineering".into(),
        members: ids.iter().map(|id| (*id).into()).collect(),
        starters: starters.iter().map(|id| (*id).into()).collect(),
        opened_at,
    }
}

pub(super) fn seats(turns: &[Turn]) -> Vec<(&str, Option<Sequence>)> {
    turns
        .iter()
        .map(|turn| (turn.seat.as_str(), turn.thread()))
        .collect()
}

pub(super) fn two_seat<'a>(
    driver: &'a CompletionDriver<'a, Seat>,
    routing: BroadcastRouting<'a>,
    policy: ConductPolicy,
    journal: &Journal,
) -> Conductor<'a, Seat> {
    Conductor::open(
        driver,
        routing,
        policy,
        door(&["one", "two"], &["one"], journal),
    )
    .expect("opens")
}
