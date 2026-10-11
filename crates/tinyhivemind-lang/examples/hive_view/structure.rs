//! The `--json` structure: one object describing a lowered package.

use serde::Serialize;
use serde_json::{Value, json};
use tinyhivemind_lang::Link;

use crate::Result;
use crate::hive::desk_policies;
use crate::load::{Loaded, body, roles, seat_desks};
use crate::math::problems;

#[derive(Debug, Serialize)]
struct RoleJson {
    id: String,
    charter: String,
}

#[derive(Debug, Serialize)]
struct SeatJson {
    id: String,
    label: String,
    role: String,
    desks: Vec<String>,
    /// Bound memory namespace, when the seat has one (an addition).
    memory: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeskJson {
    id: String,
    name: String,
    description: Option<String>,
    members: Vec<String>,
    responder_mode: String,
    /// Addition: the policy this desk's own floor runs under — the package's
    /// for one desk, its share of it for several.
    episode: DeskEpisodeJson,
}

#[derive(Debug, Serialize)]
struct DeskEpisodeJson {
    turn_budget: u32,
    round_width: u32,
    revealed_width: u32,
    quorum_threshold: u32,
}

#[derive(Debug, Serialize)]
struct GoalJson {
    id: String,
    description: String,
    subgoals: Vec<String>,
}

#[derive(Debug, Serialize)]
struct EpisodeJson {
    turn_budget: u32,
    round_width: u32,
    revealed_width: u32,
    blind_round: bool,
    quorum_threshold: u32,
    /// Additions: the rest of the quorum policy.
    quorum_window: u32,
    require_grounded: bool,
}

#[derive(Debug, Serialize)]
struct StructureJson {
    id: String,
    name: String,
    summary: String,
    roles: Vec<RoleJson>,
    seats: Vec<SeatJson>,
    desks: Vec<DeskJson>,
    topology: Vec<Link>,
    referrals: Vec<Link>,
    goals: Vec<GoalJson>,
    episode: EpisodeJson,
    sim: Value,
    /// Addition: missions as `{id, role, goals}`.
    missions: Vec<tinyhivemind_lang::Mission>,
}

/// The structure object for a loaded package.
///
/// # Errors
///
/// Fails when a math package's problem bank does not decode.
pub(crate) fn structure(loaded: &Loaded) -> Result<Value> {
    let hive = &loaded.lowered;
    let role_of = roles(hive);
    let desks_of = seat_desks(hive);
    let episode = &hive.policies.episode;
    let sizes: Vec<usize> = hive.desks.iter().map(|desk| desk.members.len()).collect();
    let seats: usize = sizes.iter().sum();
    let structure = StructureJson {
        id: hive.id.clone(),
        name: hive.name.clone(),
        summary: loaded.summary.clone(),
        roles: loaded
            .package
            .manifest
            .roles
            .iter()
            .map(|role| RoleJson {
                id: role.id.clone(),
                charter: loaded
                    .package
                    .documents
                    .get(&role.charter)
                    .and_then(|text| body(text).lines().find(|line| !line.trim().is_empty()))
                    .unwrap_or("")
                    .trim()
                    .to_owned(),
            })
            .collect(),
        seats: hive
            .seats
            .iter()
            .map(|seat| SeatJson {
                id: seat.id.clone(),
                label: seat.label.clone(),
                role: role_of
                    .get(seat.id.as_str())
                    .copied()
                    .unwrap_or("")
                    .to_owned(),
                desks: desks_of
                    .get(seat.id.as_str())
                    .map(|desks| desks.iter().map(|desk| (*desk).to_owned()).collect())
                    .unwrap_or_default(),
                memory: seat.memory.as_ref().map(|memory| memory.namespace.clone()),
            })
            .collect(),
        desks: hive
            .desks
            .iter()
            .zip(desk_policies(episode, &sizes))
            .map(|(desk, split)| DeskJson {
                id: desk.id.clone(),
                name: desk.name.clone(),
                description: desk.description.clone(),
                members: desk.members.clone(),
                responder_mode: format!("{:?}", desk.responder_mode).to_lowercase(),
                episode: {
                    let own = if hive.desks.len() > 1 {
                        split
                    } else {
                        *episode
                    };
                    DeskEpisodeJson {
                        turn_budget: own.turn_budget,
                        round_width: own.round_width,
                        revealed_width: own.revealed_width,
                        quorum_threshold: own.quorum.threshold,
                    }
                },
            })
            .collect(),
        topology: hive.topology.clone(),
        referrals: hive.referrals.clone(),
        goals: hive
            .goals
            .iter()
            .map(|goal| GoalJson {
                id: goal.id.clone(),
                description: goal.description.clone(),
                subgoals: goal.subgoals.clone(),
            })
            .collect(),
        episode: EpisodeJson {
            turn_budget: episode.turn_budget,
            round_width: episode.round_width,
            revealed_width: episode.revealed_width,
            blind_round: episode.blind_round,
            quorum_threshold: episode.quorum.threshold,
            quorum_window: episode.quorum.window,
            require_grounded: episode.quorum.require_grounded,
        },
        sim: sim(loaded)?,
        missions: hive.missions.clone(),
    };
    Ok(serde_json::to_value(structure)?)
}

fn sim(loaded: &Loaded) -> Result<Value> {
    let Some(sim) = &loaded.sim else {
        return Ok(json!({"kind": "script", "scripted": loaded.script.is_some()}));
    };
    let problems: Vec<Value> = problems(sim)?
        .into_iter()
        .map(|problem| json!({"id": problem.id, "statement": problem.statement}))
        .collect();
    Ok(json!({"kind": "math", "problems": problems}))
}
