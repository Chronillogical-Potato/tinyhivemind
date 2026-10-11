//! Offline episodes: scripted, seeded and simulated, and their determinism.

use crate::Result;
use crate::episode::{Ending, Event};
use crate::load::load;
use crate::run::{Plan, execute, prepare};

use super::support::{ending, gallery, knobs};

#[test]
fn review_board_converges_on_merge_under_its_script() -> Result<()> {
    for seed in 1..=3 {
        assert_eq!(
            ending("review-board", seed)?,
            Ending::Converged("merge".into())
        );
    }
    Ok(())
}

#[test]
fn the_seeded_default_script_ends_differently_on_different_seeds() -> Result<()> {
    let endings: Vec<Ending> = (1..=3)
        .map(|seed| ending("incident-response", seed))
        .collect::<Result<_>>()?;
    assert!(
        endings.windows(2).any(|pair| pair[0] != pair[1]),
        "{endings:?}"
    );
    // And the same seed always ends the same way.
    assert_eq!(ending("incident-response", 2)?, endings[1]);
    Ok(())
}

#[test]
fn a_math_swarm_episode_ends_within_its_turn_budget() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    for seed in 1..=10 {
        let mut prepared = prepare(&loaded, &knobs(seed))?;
        assert_eq!(prepared.seats(), 50);
        let Plan::Hive(hive) = &prepared.plan else {
            return Err("math-swarm runs one floor per desk".into());
        };
        let budget: u32 = hive.rooms.iter().map(|room| room.policy.turn_budget).sum();
        let mut widest = 0;
        let mut answers = 0;
        let outcome = execute(&mut prepared, None, &mut |event| {
            if let Event::Round(round) = event {
                widest = widest.max(round.turns.len());
                answers += usize::from(round.phase.is_none());
            }
        })?;
        assert_eq!(answers, 0, "the simulation never asks another desk");
        assert!(
            outcome.turns <= budget,
            "seed {seed}: {} turns over {budget}",
            outcome.turns
        );
        assert!(widest <= 10, "a round wider than round_width");
        assert_ne!(outcome.ending, Ending::Idle);
    }
    Ok(())
}

#[test]
fn the_same_seed_replays_the_same_frames() -> Result<()> {
    let loaded = load(&gallery("math-panel"))?;
    let transcript = |seed| -> Result<Vec<String>> {
        let mut prepared = prepare(&loaded, &knobs(seed))?;
        let mut lines = Vec::new();
        execute(&mut prepared, None, &mut |event| {
            if let Event::Round(round) = event {
                lines.extend(round.turns.iter().map(|turn| turn.content.clone()));
            }
        })?;
        Ok(lines)
    };
    assert_eq!(transcript(7)?, transcript(7)?);
    Ok(())
}
