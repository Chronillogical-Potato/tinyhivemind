//! The math simulation: seeded seats with role behaviour from `sim.json`.
//!
//! Every seat derives a private answer at the start of the episode, right with
//! a probability drawn per seat from its role's range, and then acts on what it
//! can see through core's projection. Every line it writes is the trace
//! grammar core reads, so the ending is core's quorum and nothing else.
//!
//! Behaviours (`sim.json` `roles.<role>.behaviour`):
//!
//! - `guess` proposes its guess blind, then supports the leading topic once
//!   (herding — it has no way to check).
//! - `solve` proposes its derivation, then objects to one advocate of a rival
//!   answer per turn and supports its own answer once someone else has.
//! - `check` is `solve` with up to two objections per turn.
//! - `slice` knows the answer exactly with its role's probability; otherwise it
//!   knows the answer only modulo its slice. It never claims more: it objects
//!   to advocates of answers its slice rules out and supports the leading
//!   answer its slice admits.
//! - `lead` proposes its derivation, then supports the leading topic once.
//!
//! In the commit phase every seat records the topic that carried in its view.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;
use tinyhivemind_core::hive::{
    EpisodePolicy, HiveTurn, Phase, SessionMessage, TraceKind, Visibility, trace::read,
};

use crate::Result;
use crate::episode::Speaker;
use crate::math::Problem;
use crate::rng::{PPM, Rng};
use crate::script::{QUIET, advocacy_of, commit_line, view};

/// One role's simulated behaviour.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RoleSim {
    /// `guess`, `solve`, `check`, `slice` or `lead`.
    pub(crate) behaviour: String,
    /// Range, in parts per million, a seat's accuracy is drawn from.
    pub(crate) accuracy: [u64; 2],
}

/// How a wrong derivation chooses its wrong answer, in parts per million.
#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct Errors {
    /// The problem's classic mistake.
    pub(crate) classic: u64,
    /// The answer plus one.
    pub(crate) plus_one: u64,
    /// The answer minus one.
    pub(crate) minus_one: u64,
}

/// The single seat `--compare` runs alone.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Baseline {
    /// Its role name.
    pub(crate) role: String,
    /// Its behaviour and accuracy range.
    #[serde(flatten)]
    pub(crate) sim: RoleSim,
}

/// Everything in `sim.json` except the problems.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SimConfig {
    /// Behaviour per role id.
    pub(crate) roles: BTreeMap<String, RoleSim>,
    /// Wrong-answer mix.
    pub(crate) errors: Errors,
    /// Moduli a partial calculator may know, by its index within its role.
    pub(crate) slices: Vec<u64>,
    /// The single-seat baseline.
    pub(crate) baseline: Baseline,
}

impl SimConfig {
    /// Decode `sim.json`.
    ///
    /// # Errors
    ///
    /// Fails on a malformed document or an empty slice list.
    pub(crate) fn from(sim: &Value) -> Result<Self> {
        let config: Self = serde_json::from_value(sim.clone())?;
        if config.slices.iter().any(|modulus| *modulus < 2) || config.slices.is_empty() {
            return Err("sim.json slices must be moduli of at least two".into());
        }
        Ok(config)
    }
}

#[derive(Clone, Debug)]
struct SeatState {
    behaviour: String,
    /// The full answer this seat derived, if it derived one.
    own: Option<u64>,
    /// `(modulus, residue)` a partial calculator knows instead.
    residue: Option<(u64, u64)>,
    supported: BTreeSet<String>,
    objected: BTreeSet<u64>,
}

impl SeatState {
    fn admits(&self, topic: &str) -> bool {
        let Ok(value) = topic.parse::<u64>() else {
            return false;
        };
        match (self.own, self.residue) {
            (Some(own), _) => value == own,
            (None, Some((modulus, residue))) => value % modulus == residue,
            (None, None) => true,
        }
    }
}

/// The simulated room for one problem and one seed.
#[derive(Debug)]
pub(crate) struct MathSim {
    answer: u64,
    seats: BTreeMap<String, SeatState>,
}

impl MathSim {
    /// Draw every seat's accuracy and private derivation.
    ///
    /// `seats` is `(seat id, role id)` in desk order.
    ///
    /// # Errors
    ///
    /// Fails when a seat's role has no behaviour in `sim.json`, or the problem
    /// has no computable answer.
    pub(crate) fn new(config: &SimConfig, problem: &Problem, seed: u64, seats: &[(String, String)]) -> Result<Self> {
        let answer = problem.answer()?;
        let classic = problem.classic_mistake()?;
        let mut states = BTreeMap::new();
        let mut index_in_role: BTreeMap<&str, usize> = BTreeMap::new();
        for (seat, role) in seats {
            let sim = if role == &config.baseline.role && !config.roles.contains_key(role) {
                &config.baseline.sim
            } else {
                config.roles.get(role).ok_or_else(|| format!("sim.json has no behaviour for role {role}"))?
            };
            let index = index_in_role.entry(role.as_str()).or_default();
            let modulus = config.slices[*index % config.slices.len()];
            *index += 1;
            let accuracy = Rng::stream(seed, &format!("accuracy/{seat}")).between(sim.accuracy[0], sim.accuracy[1]);
            let mut draw = Rng::stream(seed, &format!("answer/{}/{seat}", problem.id));
            let right = draw.chance(accuracy);
            let wrong = || wrong_answer(&mut draw.clone(), &config.errors, answer, classic);
            let (own, residue) = match (sim.behaviour.as_str(), right) {
                (_, true) => (Some(answer), None),
                ("slice", false) => (None, Some((modulus, answer % modulus))),
                (_, false) => (Some(wrong()), None),
            };
            states.insert(
                seat.clone(),
                SeatState {
                    behaviour: sim.behaviour.clone(),
                    own,
                    residue,
                    supported: BTreeSet::new(),
                    objected: BTreeSet::new(),
                },
            );
        }
        Ok(Self { answer, seats: states })
    }

    /// The ground-truth answer.
    pub(crate) fn answer(&self) -> u64 {
        self.answer
    }
}

fn wrong_answer(rng: &mut Rng, errors: &Errors, answer: u64, classic: u64) -> u64 {
    // Skip the draw already spent on right-or-wrong so the error pick is
    // independent of it.
    rng.next_u64();
    let total = (errors.classic + errors.plus_one + errors.minus_one).max(1);
    let roll = rng.below(total);
    if roll < errors.classic {
        classic
    } else if roll < errors.classic + errors.plus_one || answer == 0 {
        answer + 1
    } else {
        answer - 1
    }
}

impl Speaker for MathSim {
    fn speak(&mut self, turn: &HiveTurn, visible: &[SessionMessage], policy: &EpisodePolicy) -> String {
        if turn.phase == Phase::Commit {
            return commit_line(visible, policy).unwrap_or_else(|| QUIET.to_owned());
        }
        let Some(seat) = self.seats.get_mut(&turn.agent_id) else {
            return QUIET.to_owned();
        };
        if turn.visibility == Visibility::Blind {
            return opening(seat);
        }
        let lines = revealed(seat, &turn.agent_id, visible, policy);
        if lines.is_empty() { QUIET.to_owned() } else { lines.join("\n") }
    }
}

fn opening(seat: &SeatState) -> String {
    match (seat.own, seat.residue) {
        (Some(own), _) => format!("!propose #{own} ^1 My {} gives {own}.", seat.behaviour),
        (None, Some((modulus, residue))) => {
            format!("!evidence ^1 Partial: the answer is {residue} modulo {modulus}.")
        }
        (None, None) => QUIET.to_owned(),
    }
}

fn revealed(seat: &mut SeatState, id: &str, visible: &[SessionMessage], policy: &EpisodePolicy) -> Vec<String> {
    let standing = view(visible, policy);
    let mut ranked: Vec<(&str, usize)> = standing
        .iter()
        .map(|standing| (standing.topic.0.as_str(), standing.supporters.len()))
        .collect();
    ranked.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let grounds = own_proposal(visible, id).unwrap_or(1);
    let mut lines = Vec::new();
    let objections = match seat.behaviour.as_str() {
        "solve" => 1,
        "check" | "slice" => 2,
        _ => 0,
    };
    if objections > 0 {
        for (sequence, topic) in targets(visible, &standing, id, |topic| !seat.admits(topic)) {
            if lines.len() >= objections {
                break;
            }
            if seat.objected.insert(sequence) {
                lines.push(format!("!object >{sequence} ^{grounds} {topic} at {sequence} fails my check."));
            }
        }
    }
    let wanted = match seat.behaviour.as_str() {
        "guess" | "lead" => ranked.first().map(|(topic, _)| (*topic).to_owned()),
        _ => ranked.iter().find(|(topic, _)| seat.admits(topic)).map(|(topic, _)| (*topic).to_owned()),
    };
    if let Some(topic) = wanted
        && seat.own.is_none_or(|own| own.to_string() != topic || seat.behaviour != "guess")
        && !seat.supported.contains(&topic)
        && let Some(cited) = advocacy_of(visible, &topic, id)
    {
        let herding = matches!(seat.behaviour.as_str(), "guess" | "lead");
        if !herding || seat.supported.is_empty() {
            seat.supported.insert(topic.clone());
            lines.push(format!("!support #{topic} ^{cited} I reproduce {topic}."));
        }
    }
    lines
}

/// This seat's own proposal, its grounds for an objection.
fn own_proposal(visible: &[SessionMessage], id: &str) -> Option<u64> {
    read(visible)
        .into_iter()
        .find(|trace| trace.kind == TraceKind::Propose && trace.agent_id() == Some(id))
        .map(|trace| trace.sequence.0)
}

/// Messages advocating a rejected topic whose author is not yet silenced
/// there, most-supported topic first.
fn targets(
    visible: &[SessionMessage],
    standing: &[tinyhivemind_core::hive::TopicStanding],
    id: &str,
    rejects: impl Fn(&str) -> bool,
) -> Vec<(u64, String)> {
    let mut found: Vec<(usize, u64, String)> = Vec::new();
    for trace in read(visible) {
        if !matches!(trace.kind, TraceKind::Propose | TraceKind::Support) {
            continue;
        }
        let (Some(topic), Some(author)) = (trace.topic.as_ref(), trace.agent_id()) else {
            continue;
        };
        if author == id || !rejects(&topic.0) {
            continue;
        }
        let Some(standing) = standing.iter().find(|standing| standing.topic == *topic) else {
            continue;
        };
        if standing.silenced.iter().any(|silenced| silenced == author) {
            continue;
        }
        found.push((standing.supporters.len(), trace.sequence.0, topic.0.clone()));
    }
    found.sort_by_key(|(support, sequence, _)| (std::cmp::Reverse(*support), *sequence));
    found.into_iter().map(|(_, sequence, topic)| (sequence, topic)).collect()
}

/// Whether `ppm` is a valid probability.
pub(crate) fn valid_ppm(ppm: u64) -> bool {
    ppm <= PPM
}
