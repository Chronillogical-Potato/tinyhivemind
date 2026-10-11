//! Desk routing: one floor per desk, referrals only along declared links,
//! core's hop budget, and what this host refuses.

use std::collections::BTreeMap;

use tinyhivemind_core::hive::{EpisodePolicy, HiveTurn, SessionMessage, Visibility};

use crate::Result;
use crate::desks::{DeskEnd, FORWARD_CAP, ReferralRecord, run_hive};
use crate::episode::{Ending, Event, RoundRecord, Speaker};
use crate::hive::{Hive, desk_policy};
use crate::load::{Loaded, load};
use crate::run::{Plan, execute, prepare};

use super::support::{gallery, knobs};

/// Everything one run reported, by kind.
#[derive(Debug, Default)]
struct Seen {
    rounds: Vec<RoundRecord>,
    referrals: Vec<ReferralRecord>,
    ends: Vec<DeskEnd>,
}

fn watch(loaded: &Loaded, at: u64) -> Result<(Seen, crate::episode::Outcome)> {
    let mut prepared = prepare(loaded, &knobs(at))?;
    let mut seen = Seen::default();
    let outcome = execute(&mut prepared, None, &mut |event| match event {
        Event::Round(round) => seen.rounds.push(round.clone()),
        Event::Referral(referral) => seen.referrals.push(referral.clone()),
        Event::DeskEnd(end) => seen.ends.push(end.clone()),
    })?;
    Ok((seen, outcome))
}

fn hive_of(loaded: &Loaded) -> Result<Hive> {
    match crate::run::plan(loaded, &knobs(1), "Problem: pick a number. Cite ^1.".into())? {
        Plan::Hive(hive) => Ok(hive),
        Plan::Room(_) => Err("expected several desks".into()),
    }
}

#[test]
fn a_desk_takes_its_share_of_the_package_policy() {
    let mut policy = EpisodePolicy::default();
    policy.quorum.threshold = 26;
    policy.turn_budget = 150;
    policy.round_width = 10;
    policy.revealed_width = 12;
    let desk = desk_policy(&policy, 10, 50);
    assert_eq!(desk.quorum.threshold, 6, "26 x 10 / 50, rounded up");
    assert_eq!(desk.turn_budget, 30);
    assert_eq!((desk.round_width, desk.revealed_width), (10, 10));
    let tiny = desk_policy(&policy, 1, 50);
    assert_eq!((tiny.quorum.threshold, tiny.turn_budget), (1, 3));
}

#[test]
fn every_desk_of_the_swarm_runs_its_own_floor_and_the_last_one_answers() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let desk_of: BTreeMap<&str, &str> = loaded
        .lowered
        .desks
        .iter()
        .flat_map(|desk| {
            desk.members
                .iter()
                .map(move |member| (member.as_str(), desk.id.as_str()))
        })
        .collect();
    for seed in 1..=5 {
        let (seen, outcome) = watch(&loaded, seed)?;
        for round in &seen.rounds {
            for turn in &round.turns {
                assert_eq!(
                    desk_of.get(turn.seat.as_str()),
                    Some(&round.desk.as_str()),
                    "seed {seed}: {} spoke on {}",
                    turn.seat,
                    round.desk
                );
            }
        }
        let ended: Vec<&str> = seen.ends.iter().map(|end| end.desk.as_str()).collect();
        assert_eq!(
            ended,
            ["conjecture", "compute", "prove", "verify", "synthesis"],
            "seed {seed}: a downstream desk opens only once its upstream desks ended"
        );
        let synthesis = seen.ends.last().ok_or("a synthesis ending")?;
        let expected = match &synthesis.ending {
            Ending::Unopened => Ending::Exhausted,
            other => other.clone(),
        };
        assert_eq!(outcome.ending, expected, "seed {seed}");
        let rounds = seen.rounds.iter().map(|round| round.round);
        assert!(rounds.clone().eq(1..=outcome.rounds), "seed {seed}");
    }
    Ok(())
}

#[test]
fn referrals_land_only_along_declared_links() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let hive = hive_of(&loaded)?;
    for seed in 1..=10 {
        let (seen, _) = watch(&loaded, seed)?;
        assert!(seen.referrals.iter().any(|referral| referral.accepted));
        for referral in seen.referrals.iter().filter(|referral| referral.accepted) {
            let link = (referral.from_desk.clone(), referral.to_desk.clone());
            assert!(hive.links.contains(&link), "seed {seed}: {referral:?}");
            assert!(referral.hops <= loaded.lowered.policies.referral.max_hops);
            assert!(referral.sequence.is_some() && referral.reason.is_none());
        }
    }
    Ok(())
}

#[test]
fn a_report_past_max_hops_is_refused_by_core() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let (seen, _) = watch(&loaded, 3)?;
    let refused: Vec<&ReferralRecord> = seen
        .referrals
        .iter()
        .filter(|referral| referral.from_desk == "synthesis")
        .collect();
    assert_eq!(refused.len(), 2, "synthesis reports back along two links");
    for referral in refused {
        assert!(!referral.accepted);
        assert_eq!(referral.reason.as_deref(), Some("hop_limit_reached"));
        assert_eq!(referral.hops, loaded.lowered.policies.referral.max_hops);
    }
    // With a budget of one hop a desk that received a report cannot pass
    // one on; a desk whose upstream reports were refused starts afresh.
    let mut shallow = load(&gallery("math-swarm"))?;
    shallow.lowered.policies.referral.max_hops = 1;
    let (seen, _) = watch(&shallow, 3)?;
    for referral in &seen.referrals {
        if referral.accepted {
            assert_eq!(referral.hops, 1, "{referral:?}");
        } else {
            // Feedback to a desk that already ended is refused by this host.
            let reason = referral.reason.as_deref();
            assert!(
                matches!(reason, Some("hop_limit_reached" | "desk_closed")),
                "{referral:?}"
            );
        }
    }
    let from = |desk: &str| -> Vec<bool> {
        seen.referrals
            .iter()
            .filter(|referral| referral.from_desk == desk)
            .map(|referral| referral.accepted)
            .collect()
    };
    assert_eq!(from("conjecture"), [true, true]);
    assert_eq!(from("compute"), [false, false]);
    Ok(())
}

#[test]
fn a_disabled_policy_refuses_every_report_and_desks_decide_alone() -> Result<()> {
    let mut loaded = load(&gallery("math-swarm"))?;
    loaded.lowered.policies.referral.enabled = false;
    let (seen, _) = watch(&loaded, 2)?;
    assert!(!seen.referrals.is_empty());
    assert!(
        seen.referrals
            .iter()
            .all(|referral| !referral.accepted && referral.reason.as_deref() == Some("disabled"))
    );
    assert_eq!(seen.ends.len(), 5);
    Ok(())
}

#[test]
fn a_one_desk_package_routes_nothing_and_reports_its_own_desk() -> Result<()> {
    let loaded = load(&gallery("math-panel"))?;
    let (seen, _) = watch(&loaded, 4)?;
    assert!(seen.referrals.is_empty());
    assert_eq!(seen.ends.len(), 0);
    assert!(seen.rounds.iter().all(|round| round.desk == "panel"
        && round.desk_round == round.round
        && round.phase.is_some()));
    Ok(())
}

/// A speaker whose seats ask other desks: `asks` maps a seat to the line it
/// writes on its first turn; everyone else proposes `#7`.
struct Asking {
    asks: BTreeMap<String, String>,
    answered: Vec<(String, String)>,
}

impl Speaker for Asking {
    fn speak(
        &mut self,
        turn: &HiveTurn,
        _visible: &[SessionMessage],
        _policy: &EpisodePolicy,
    ) -> String {
        if let Some(line) = self.asks.remove(&turn.agent_id) {
            return line;
        }
        if turn.visibility == Visibility::Blind {
            "!propose #7 ^1 Seven.".into()
        } else {
            "!question Nothing further.".into()
        }
    }

    fn answer(
        &mut self,
        seat: &str,
        incoming: &str,
        _visible: &[SessionMessage],
    ) -> Result<String> {
        self.answered.push((seat.to_owned(), incoming.to_owned()));
        Ok("!evidence ^1 It is seven.".into())
    }
}

fn asking(loaded: &Loaded, asks: &[(&str, &str)]) -> Result<(Seen, Asking)> {
    let hive = hive_of(loaded)?;
    let mut speaker = Asking {
        asks: asks
            .iter()
            .map(|(seat, line)| ((*seat).to_owned(), (*line).to_owned()))
            .collect(),
        answered: Vec::new(),
    };
    let mut seen = Seen::default();
    run_hive(&hive, &mut speaker, None, &mut |event| match event {
        Event::Round(round) => seen.rounds.push(round.clone()),
        Event::Referral(referral) => seen.referrals.push(referral.clone()),
        Event::DeskEnd(end) => seen.ends.push(end.clone()),
    })?;
    Ok((seen, speaker))
}

#[test]
fn a_linked_ask_is_answered_on_the_far_desk_and_returns_home() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let (seen, speaker) = asking(
        &loaded,
        &[(
            "conjecture-01",
            "!propose #7 ^1 Seven?\n@#verify can you check seven?",
        )],
    )?;
    let forward = seen
        .referrals
        .iter()
        .find(|referral| referral.kind == "forward")
        .ok_or("the ask was routed")?;
    assert!(forward.accepted, "{forward:?}");
    assert_eq!(
        (forward.from_desk.as_str(), forward.to_desk.as_str()),
        ("conjecture", "verify")
    );
    assert_eq!(forward.hops, 1);
    let target = forward.target.clone().ok_or("core chose a seat")?;
    assert!(target.starts_with("verify-"));
    assert_eq!(speaker.answered.len(), 1);
    assert_eq!(speaker.answered[0].0, target);
    let answer = seen
        .rounds
        .iter()
        .find(|round| round.phase.is_none())
        .ok_or("an answer round")?;
    assert_eq!(answer.desk, "verify");
    assert_eq!(answer.turns[0].reason, "referral");
    let back = seen
        .referrals
        .iter()
        .find(|referral| referral.kind == "return")
        .ok_or("the answer was routed back")?;
    assert_eq!(
        (back.from_desk.as_str(), back.to_desk.as_str()),
        ("verify", "conjecture")
    );
    assert_eq!(back.hops, 2);
    assert!(
        back.accepted || back.reason.as_deref() == Some("desk_closed"),
        "{back:?}"
    );
    Ok(())
}

#[test]
fn an_ask_to_an_unlinked_desk_is_refused() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let (seen, speaker) = asking(
        &loaded,
        &[(
            "conjecture-01",
            "!propose #7 ^1 Seven.\n@#synthesis is it seven?",
        )],
    )?;
    let refused = seen
        .referrals
        .iter()
        .find(|referral| referral.kind == "forward")
        .ok_or("the ask was considered")?;
    assert!(!refused.accepted);
    assert_eq!(refused.reason.as_deref(), Some("unlinked"));
    assert_eq!(refused.to_desk, "synthesis");
    assert!(speaker.answered.is_empty());
    Ok(())
}

#[test]
fn a_desk_may_ask_only_a_constant_number_of_times() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let asks: Vec<(String, String)> = (1..=5)
        .map(|n| {
            (
                format!("conjecture-{n:02}"),
                format!("!propose #7 ^1 Seven.\n@#compute question {n}"),
            )
        })
        .collect();
    let borrowed: Vec<(&str, &str)> = asks
        .iter()
        .map(|(seat, line)| (seat.as_str(), line.as_str()))
        .collect();
    let (seen, _) = asking(&loaded, &borrowed)?;
    let forwards: Vec<&ReferralRecord> = seen
        .referrals
        .iter()
        .filter(|referral| referral.kind == "forward")
        .collect();
    assert_eq!(forwards.len(), 5);
    let accepted = forwards.iter().filter(|referral| referral.accepted).count();
    assert_eq!(accepted, FORWARD_CAP as usize);
    assert!(
        forwards
            .iter()
            .filter(|referral| !referral.accepted)
            .all(|referral| referral.reason.as_deref() == Some("width_cap"))
    );
    Ok(())
}
