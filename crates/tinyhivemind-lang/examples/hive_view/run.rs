//! Build a plan (one room, or one floor per desk) and a speaker for a package.

use tinyhivemind_core::hive::{
    EpisodePolicy, QuorumPolicy, desk::Desk, desk::ResponderMode, roster::RosterMember,
};
use tinyhivemind_core::telemetry::Tracer;

use crate::Result;
use crate::desks::{Hive, run_hive};
use crate::episode::{Ending, Event, Outcome, Room, Speaker, run};
use crate::live::LiveConfig;
use crate::live::LiveSpeaker;
use crate::load::{Loaded, roles};
use crate::math::{Problem, problems};
use crate::rng::Rng;
use crate::script::{Scripted, Seeded};
use crate::sim::{MathSim, SimConfig};

/// Run knobs from the command line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Knobs {
    pub(crate) seed: u64,
    pub(crate) round_width: Option<u32>,
    pub(crate) turn_budget: Option<u32>,
    pub(crate) problem: Option<String>,
}

/// What a run drives: one desk's room, or one floor per desk.
#[derive(Clone, Debug)]
pub(crate) enum Plan {
    /// A package of one desk (or the single-seat baseline).
    Room(Room),
    /// A package of several desks, routed by referral.
    Hive(Hive),
}

/// A plan ready to run, its speaker, and the problem it poses, if math.
pub(crate) struct Prepared {
    pub(crate) plan: Plan,
    pub(crate) speaker: Box<dyn Speaker>,
    pub(crate) problem: Option<(Problem, u64)>,
}

impl Prepared {
    /// The package id.
    pub(crate) fn package(&self) -> &str {
        match &self.plan {
            Plan::Room(room) => &room.package,
            Plan::Hive(hive) => &hive.package,
        }
    }

    /// Seats across every desk.
    pub(crate) fn seats(&self) -> usize {
        match &self.plan {
            Plan::Room(room) => room.members.len(),
            Plan::Hive(hive) => hive.seats(),
        }
    }

    /// The desk whose ending is the hive's, on a run of several desks.
    pub(crate) fn answer_desk(&self) -> Option<String> {
        match &self.plan {
            Plan::Room(_) => None,
            Plan::Hive(hive) => Some(hive.rooms[hive.answer()].desk.id.clone()),
        }
    }
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Prepared")
            .field("plan", &self.plan)
            .finish_non_exhaustive()
    }
}

/// The problem a seed poses: `--problem` if given, else a seeded pick.
///
/// # Errors
///
/// Fails when `--problem` names a problem the bank does not hold.
pub(crate) fn pick(bank: &[Problem], seed: u64, chosen: Option<&str>) -> Result<Problem> {
    if let Some(id) = chosen {
        return bank
            .iter()
            .find(|problem| problem.id == id)
            .cloned()
            .ok_or_else(|| format!("no problem {id} in the bank").into());
    }
    let index = usize::try_from(Rng::stream(seed, "problem").below(bank.len() as u64))?;
    bank.get(index)
        .cloned()
        .ok_or_else(|| "empty problem bank".into())
}

fn apply(policy: &mut EpisodePolicy, knobs: &Knobs) {
    if let Some(width) = knobs.round_width {
        policy.round_width = width;
        policy.revealed_width = width;
    }
    if let Some(budget) = knobs.turn_budget {
        policy.turn_budget = budget;
    }
}

fn opening_for(loaded: &Loaded) -> String {
    let hive = &loaded.lowered;
    let ask = hive
        .goals
        .first()
        .map(|goal| goal.description.clone())
        .or_else(|| hive.desks.first().and_then(|desk| desk.description.clone()))
        .unwrap_or_else(|| hive.name.clone());
    format!("{ask}. Deliberate with !propose, !support and !object, citing with ^N.")
}

fn math_opening(problem: &Problem) -> String {
    format!(
        "Problem {}: {} Propose your answer as #<answer> citing ^1.",
        problem.id, problem.statement
    )
}

/// The plan for a package: its one desk, or one floor per desk. Knobs apply
/// to the package's policy, which a hive then shares out among its desks.
///
/// # Errors
///
/// Fails on a package with no desk.
pub(crate) fn plan(loaded: &Loaded, knobs: &Knobs, opening: String) -> Result<Plan> {
    let lowered = &loaded.lowered;
    let mut policy = lowered.policies.episode;
    apply(&mut policy, knobs);
    match lowered.desks.as_slice() {
        [] => Err(format!("{} declares no desk", lowered.id).into()),
        [only] => Ok(Plan::Room(Room::of(lowered, only, policy, opening))),
        _ => Ok(Plan::Hive(Hive::of(lowered, &policy, &opening))),
    }
}

/// Every seat of a plan in desk order, with its role.
fn seat_roles(loaded: &Loaded, plan: &Plan) -> Vec<(String, String)> {
    let role_of = roles(&loaded.lowered);
    let members = match plan {
        Plan::Room(room) => &room.members,
        Plan::Hive(hive) => &hive.everyone,
    };
    members
        .iter()
        .map(|member| {
            (
                member.id.clone(),
                role_of
                    .get(member.id.as_str())
                    .copied()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect()
}

/// Prepare a package's run for these knobs, offline.
///
/// # Errors
///
/// Fails on a malformed `sim.json`, an unknown `--problem`, or no desk.
pub(crate) fn prepare(loaded: &Loaded, knobs: &Knobs) -> Result<Prepared> {
    prepare_with(loaded, knobs, None)
}

/// Prepare a run, live when `live` names a model and its run label.
///
/// A live run poses the same problem the offline one would, and lets a
/// model play every seat; without a math simulation it is posed the
/// package's opening.
///
/// # Errors
///
/// As [`prepare`], or as [`LiveSpeaker::new`].
pub(crate) fn prepare_with(
    loaded: &Loaded,
    knobs: &Knobs,
    live: Option<(&LiveConfig, &str)>,
) -> Result<Prepared> {
    let Some(sim) = &loaded.sim else {
        let plan = plan(loaded, knobs, opening_for(loaded))?;
        let speaker: Box<dyn Speaker> = match (live, &loaded.script) {
            (Some((config, run)), _) => Box::new(LiveSpeaker::new(loaded, config, knobs.seed, run)?),
            (None, Some(script)) => Box::new(Scripted::new(script)),
            (None, None) => Box::new(Seeded::new(knobs.seed, seat_roles(loaded, &plan).len())),
        };
        return Ok(Prepared {
            plan,
            speaker,
            problem: None,
        });
    };
    let config = SimConfig::from(sim)?;
    let problem = pick(&problems(sim)?, knobs.seed, knobs.problem.as_deref())?;
    let plan = plan(loaded, knobs, math_opening(&problem))?;
    let simulation = MathSim::new(&config, &problem, knobs.seed, &seat_roles(loaded, &plan))?;
    let answer = simulation.answer();
    let speaker: Box<dyn Speaker> = match live {
        Some((config, run)) => Box::new(LiveSpeaker::new(loaded, config, knobs.seed, run)?),
        None => Box::new(simulation),
    };
    Ok(Prepared {
        plan,
        speaker,
        problem: Some((problem, answer)),
    })
}

/// The single-seat baseline: one seat of the baseline role answering alone.
///
/// Its episode still runs through core: a one-member desk, quorum of one,
/// three turns — propose, then record.
///
/// # Errors
///
/// As [`prepare`].
pub(crate) fn baseline(loaded: &Loaded, knobs: &Knobs) -> Result<Prepared> {
    let sim = loaded
        .sim
        .as_ref()
        .ok_or("the baseline needs a math package")?;
    let config = SimConfig::from(sim)?;
    let problem = pick(&problems(sim)?, knobs.seed, knobs.problem.as_deref())?;
    let seat = "solo".to_owned();
    let desk = Desk {
        id: "solo".into(),
        name: "Single seat".into(),
        description: Some("One seat answering alone".into()),
        members: vec![seat.clone()],
        responder_mode: ResponderMode::Auto,
    };
    let policy = EpisodePolicy {
        turn_budget: 3,
        round_width: 1,
        revealed_width: 1,
        quorum: QuorumPolicy {
            threshold: 1,
            window: 3,
            require_grounded: true,
        },
        ..loaded.lowered.policies.episode
    };
    let mut lone = config.clone();
    lone.roles.clear();
    let simulation = MathSim::new(
        &lone,
        &problem,
        knobs.seed,
        &[(seat.clone(), config.baseline.role.clone())],
    )?;
    let answer = simulation.answer();
    let room = Room {
        package: "single-seat".into(),
        members: vec![RosterMember {
            id: seat.clone(),
            name: Some(seat.clone()),
        }],
        home: [(seat.clone(), desk.id.clone())].into(),
        desk,
        policy,
        opening: math_opening(&problem),
    };
    Ok(Prepared {
        plan: Plan::Room(room),
        speaker: Box::new(simulation),
        problem: Some((problem, answer)),
    })
}

/// Run a prepared plan, calling `observe` per round, referral and desk end.
///
/// # Errors
///
/// As [`run`] and [`run_hive`].
pub(crate) fn execute(
    prepared: &mut Prepared,
    tracer: Option<&Tracer<'_>>,
    observe: &mut dyn FnMut(Event<'_>),
) -> Result<Outcome> {
    match &prepared.plan {
        Plan::Room(room) => run(room, prepared.speaker.as_mut(), tracer, observe),
        Plan::Hive(hive) => run_hive(hive, prepared.speaker.as_mut(), tracer, observe),
    }
}

/// Whether a math outcome decided the right answer; `None` when not math.
pub(crate) fn correct(outcome: &Outcome, problem: Option<&(Problem, u64)>) -> Option<bool> {
    problem.map(|(_, answer)| outcome.ending.topic() == Some(answer.to_string().as_str()))
}

