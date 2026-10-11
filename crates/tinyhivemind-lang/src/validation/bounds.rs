//! Policy and coordinator bounds against the constitution's caps.
//!
//! Each failing bound is located at the `hive.json` field an editor would
//! change, except a zero cap, which lives in `constitution.json`.
use crate::{
    Error, Package,
    diagnostic::{CONSTITUTION, Findings, HIVE},
};

/// Record a failed bound named `field` at `pointer` in `file`.
fn bound(found: &mut Findings, field: &str, valid: bool, file: &str, pointer: &str) {
    if !valid {
        found.push(
            Error::Bound {
                field: field.into(),
            },
            file,
            pointer.to_owned(),
        );
    }
}

fn widen(width: usize) -> u64 {
    u64::try_from(width).unwrap_or(u64::MAX)
}

pub(super) fn bounds(package: &Package, found: &mut Findings) {
    let p = &package.manifest.policies;
    let c = &package.constitution;
    let settings = &package.manifest.coordinator;
    let cap = if c.max_turns == 0 {
        "/max_turns"
    } else {
        "/max_width"
    };
    bound(
        found,
        "constitution",
        c.max_turns > 0 && c.max_width > 0,
        CONSTITUTION,
        cap,
    );
    bound(
        found,
        "episode.turn_budget",
        p.episode.turn_budget > 0 && u64::from(p.episode.turn_budget) <= c.max_turns,
        HIVE,
        "/policies/episode/turn_budget",
    );
    let width = if p.episode.round_width > 0 && p.episode.round_width <= p.episode.revealed_width {
        "/policies/episode/revealed_width"
    } else {
        "/policies/episode/round_width"
    };
    bound(
        found,
        "episode.width",
        p.episode.round_width > 0
            && p.episode.round_width <= p.episode.revealed_width
            && u64::from(p.episode.revealed_width) <= widen(c.max_width),
        HIVE,
        width,
    );
    bound(
        found,
        "episode.dominance_cap",
        p.episode.dominance_cap <= 100,
        HIVE,
        "/policies/episode/dominance_cap",
    );
    rounds(package, found);
    policies(package, found);
}

/// Division, routing, coordinator and conduct widths and walls.
fn rounds(package: &Package, found: &mut Findings) {
    let p = &package.manifest.policies;
    let c = &package.constitution;
    let settings = &package.manifest.coordinator;
    bound(
        found,
        "division.round_width",
        p.division.round_width > 0 && u64::from(p.division.round_width) <= widen(c.max_width),
        HIVE,
        "/policies/division/round_width",
    );
    bound(
        found,
        "routing",
        p.routing.round_width > 0
            && p.routing.round_width <= c.max_width
            && p.routing.choice_option_limit > 0,
        HIVE,
        "/policies/routing",
    );
    bound(
        found,
        "routing.probability",
        [
            p.routing.minimum_confidence,
            p.routing.high_impact_minimum_confidence,
            p.routing.clarification_threshold,
            p.routing.high_impact_threshold,
        ]
        .iter()
        .all(|value| value.parts() <= 1_000_000),
        HIVE,
        "/policies/routing",
    );
    bound(
        found,
        "coordinator.round_width",
        settings.round_width > 0 && settings.round_width <= c.max_width,
        HIVE,
        "/coordinator/round_width",
    );
    for (policy, pointer) in [
        (&p.conduct, "/policies/conduct"),
        (&settings.conduct_policy, "/coordinator/conduct_policy"),
    ] {
        bound(
            found,
            "conduct",
            policy.child_turn_wall > 0
                && policy.turn_wall > 0
                && policy.child_turn_wall <= policy.turn_wall
                && policy.turn_wall <= c.max_turns,
            HIVE,
            pointer,
        );
    }
    bound(
        found,
        "broadcast_budget",
        settings.broadcast_budget != Some(0),
        HIVE,
        "/coordinator/broadcast_budget",
    );
}

/// The remaining policies' nonzero budgets and enabled-feature caps.
fn policies(package: &Package, found: &mut Findings) {
    let p = &package.manifest.policies;
    bound(
        found,
        "digest",
        p.digest.input_limit > 0 && p.digest.budget_chars > 0,
        HIVE,
        "/policies/digest",
    );
    bound(
        found,
        "aside",
        !p.aside.enabled || (p.aside.max_members > 0 && p.aside.max_messages > 0),
        HIVE,
        "/policies/aside",
    );
    bound(
        found,
        "quorum",
        p.episode.quorum.threshold > 0 && p.episode.quorum.window > 0,
        HIVE,
        "/policies/episode/quorum",
    );
    bound(
        found,
        "episode.repetition_cap",
        p.episode.repetition_cap > 0,
        HIVE,
        "/policies/episode/repetition_cap",
    );
    let half_life = if p.episode.weights.half_life == 0 {
        "/policies/episode/weights/half_life"
    } else {
        "/policies/directory/half_life"
    };
    bound(
        found,
        "half_life",
        p.episode.weights.half_life > 0 && p.directory.half_life > 0,
        HIVE,
        half_life,
    );
    bound(
        found,
        "referral",
        !p.referral.enabled || p.referral.max_hops > 0,
        HIVE,
        "/policies/referral",
    );
    bound(
        found,
        "brevity",
        p.brevity.message_chars > 0 && p.brevity.window > 0,
        HIVE,
        "/policies/brevity",
    );
    bound(
        found,
        "exchange",
        !p.exchange.enabled || (p.exchange.contact_cap > 0 && p.exchange.round_cap > 0),
        HIVE,
        "/policies/exchange",
    );
}
