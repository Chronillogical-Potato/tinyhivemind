//! Resume from every snapshot and compare with the uninterrupted run.

use std::collections::BTreeMap;

use tinyhivemind_core::runtime::LogMessage;
use tinyhivemind_lab::{Res, TraceRig, section};

use crate::conduct::{Scenario, play};

type Rows = Vec<(u64, String, Option<u64>, String)>;

/// Rows with their sequence numbers erased, as a multiset.
fn multiset(rows: &Rows) -> BTreeMap<(String, Option<u64>, String), usize> {
    let mut counted = BTreeMap::new();
    for (_, author, thread, content) in rows {
        *counted
            .entry((author.clone(), *thread, content.clone()))
            .or_default() += 1;
    }
    counted
}

pub fn run(rig: &TraceRig) -> Res {
    section("snapshot and resume: replay from every commit boundary");
    println!(
        "  a snapshot is taken after every committed row; each is resumed over the log as it stood"
    );
    println!(
        "  {:<26} {:>9} {:>9} {:>9} {:>9} {:>11}",
        "scenario", "snapshots", "identical", "reordered", "different", "extra turns"
    );
    for (label, scn) in [
        ("baseline", Scenario::default()),
        (
            "chatty tester, width 1",
            Scenario {
                driver_width: 1,
                chatty_tester: true,
                ..Scenario::default()
            },
        ),
        (
            "coder parked",
            Scenario {
                park: true,
                ..Scenario::default()
            },
        ),
        (
            "coder is a starter",
            Scenario {
                queue_work: true,
                ..Scenario::default()
            },
        ),
        (
            "planner spams, budget 1",
            Scenario {
                spam: true,
                budget: Some(1),
                ..Scenario::default()
            },
        ),
    ] {
        let whole = play(&scn, &rig.tracer(&format!("replay:{label}")), true, None);
        let silent = rig.tracer("replay:resumed");
        let (mut identical, mut reordered, mut different, mut extra) = (0, 0, 0, 0_i64);
        for (json, kept) in &whole.snapshots {
            let rows: &[LogMessage] = &whole.log[..*kept];
            let resumed = play(&scn, &silent, false, Some((json, rows)));
            extra +=
                i64::try_from(resumed.turns).unwrap_or(0) - i64::try_from(whole.turns).unwrap_or(0);
            if resumed.rows == whole.rows && resumed.error.is_none() {
                identical += 1;
            } else if multiset(&resumed.rows) == multiset(&whole.rows) {
                reordered += 1;
            } else {
                different += 1;
            }
        }
        println!(
            "  {label:<26} {:>9} {identical:>9} {reordered:>9} {different:>9} {extra:>11}",
            whole.snapshots.len()
        );
    }
    println!(
        "  Seen::ran_for is not persisted, so a resumed seat is owed one more turn than the run it resumes."
    );
    println!(
        "  `turns` here is the resumed conductor's own count, which starts from the snapshot's."
    );
    Ok(())
}
