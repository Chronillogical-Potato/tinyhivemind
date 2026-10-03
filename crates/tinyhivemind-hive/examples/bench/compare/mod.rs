//! The simulated multi-arm comparison engine.
//!
//! `compare` is what `cargo run --example bench` does with no flags: it runs
//! every arm — the ladder, the matched-budget vote, the crate default, the
//! tuned policy and its delegation and check-arm variants, and the earned
//! directory the ladder is handed — over the same sample of rooms, and prints
//! the resulting table. Kept apart from `main.rs` because this is the one
//! function with the most to say about what the benchmark measures, and it
//! says most of it through the doc comments on [`Totals`]'s fields.

mod totals;

use std::time::Instant;

use tinyhivemind_hive::EpisodePolicy;

use crate::TASK;
use crate::arms;
use crate::cli::Options;
use crate::metrics::{
    Aggregate, arm_header, arm_row, detail_header, detail_row, json_line, library_header,
    library_row, paired_against, paired_diff_line,
};
use crate::parallel;
use crate::policy::{blind_wide_policy, default_policy, widened_policy};
use crate::rng::mix;
use crate::run::{AsideMode, run_episode, run_episode_checking, run_episode_exchanging_with};
use crate::sim::{CheckStyle, Room, SPECIALIST_COST_UNIT};
use totals::Totals;

/// Run every arm over the same rooms and print the comparison.
pub(crate) fn compare(options: &Options, rooms: &[Room]) -> Result<(), String> {
    let tuned = options.policy;
    println!("{}\n", options.cost_model.header());
    println!(
        "rooms {}  agents {}  options {}  eval noise ±{}\n\
         tuned policy: budget {}  quorum {}  blind {}  dominance {}  repetition {}\n",
        rooms.len(),
        options.agents,
        options.topics,
        options.noise,
        tuned.turn_budget,
        tuned.quorum.threshold,
        if tuned.blind_round { "yes" } else { "no" },
        tuned.dominance_cap,
        tuned.repetition_cap,
    );

    let (totals, wall) = run_arms(options, rooms)?;
    let arms: [(&str, &Aggregate); 18] = [
        ("ladder", &totals.ladder),
        ("vote", &totals.vote),
        ("hive", &totals.hive_default),
        ("hive+", &totals.hive_tuned),
        // Appended rather than interleaved: the six rows above are the
        // previous table, and the paired-bootstrap seed below is derived
        // from an arm's index in this list.
        ("hive+aside", &totals.hive_aside),
        ("hive+ask", &totals.hive_ask),
        ("hive+aside!", &totals.hive_aside_informed),
        ("hive+fact", &totals.hive_aside_fact),
        ("hive+mute", &totals.hive_aside_mute),
        ("hive+along", &totals.hive_aside_alongside),
        ("hive+share", &totals.hive_aside_exchange),
        ("hive+hush", &totals.hive_aside_hush),
        ("hive+rounds", &totals.hive_exchange_rounds),
        ("hive+quiet", &totals.hive_exchange_quiet),
        ("hive+fact°", &totals.hive_aside_offfloor),
        ("hive+pooled", &totals.hive_pooled),
        ("hive+wide", &totals.hive_wide),
        ("hive+blind", &totals.hive_blind_wide),
    ];

    if options.json {
        for (name, arm) in arms {
            println!("{}", json_line(name, arm));
        }
        if options.cost {
            println!("{}", json_line("all-reasoning", &totals.all_reasoning));
        }
    }

    println!("{}", arm_header());
    for (name, arm) in arms {
        println!("{}", arm_row(name, arm));
    }

    // The library's own cost, under a heading that says so. It used to sit in
    // the table above, where `ns/step 0` and `episodes/s inf` on the `vote`
    // row read as "this arm is free" rather than "this arm never calls the
    // library" -- which is what they mean.
    println!("\nwhat the library itself costs, with every agent's time excluded");
    println!("{}", library_header());
    for (name, arm) in arms {
        println!("{}", library_row(name, arm));
    }

    println!("\n{}", detail_header());
    for (name, arm) in arms {
        println!("{}", detail_row(name, arm));
    }
    for (index, (name, arm)) in arms.iter().filter(|(name, _)| *name != "vote").enumerate() {
        let seed = mix(options.seed, 0xB007_57AA_u64.wrapping_add(index as u64));
        if let Some(line) = paired_diff_line(name, arm, &totals.vote, seed, 2000) {
            println!("{line}");
        }
    }

    check_arm_diffs(options, &totals);

    if options.cost {
        cost_table(&[
            ("vote", &totals.vote),
            ("ladder", &totals.ladder),
            ("hive+", &totals.hive_tuned),
            ("all-reasoning", &totals.all_reasoning),
        ]);
    }

    endings(&totals);
    println!(
        "library time {:.1} ms over {} steps ({:.0} ns/step, {:.0} episodes/s)",
        totals.hive_tuned.library_time.as_secs_f64() * 1_000.0,
        totals.hive_tuned.step_calls,
        totals.hive_tuned.nanos_per_step(),
        totals.hive_tuned.episodes_per_second(),
    );
    println!(
        "wall clock {:.1} ms for {} rooms across every arm",
        wall.as_secs_f64() * 1_000.0,
        rooms.len(),
    );
    Ok(())
}

/// Run every arm over the same rooms, and say how long the whole sample took.
///
/// # Errors
///
/// Returns the library's own error text from any arm.
/// Run every arm that turns on a pairwise check, over one room.
///
/// Split out of [`run_arms`] because there are now seven of them and they
/// form one experiment: three that vary who reads the answer and where the
/// question is aimed, one matched-turn control that throws the answer away,
/// one that holds the same exchange off the floor, and one free ceiling. Read
/// together they separate what an exchange is worth from what its turns cost;
/// read one at a time they do not.
///
/// # Errors
///
/// Returns the library's own error text from any arm.
/// Print the check arms against the room they modify, rather than against the
/// poll.
///
/// Seeded off a tag of their own so the published bootstraps keep their
/// streams.
fn check_arm_diffs(options: &Options, totals: &Totals) {
    for (index, (name, arm)) in [
        ("hive+aside", &totals.hive_aside),
        ("hive+ask", &totals.hive_ask),
        ("hive+aside!", &totals.hive_aside_informed),
        ("hive+fact", &totals.hive_aside_fact),
        ("hive+mute", &totals.hive_aside_mute),
        ("hive+along", &totals.hive_aside_alongside),
        ("hive+share", &totals.hive_aside_exchange),
        ("hive+hush", &totals.hive_aside_hush),
        ("hive+rounds", &totals.hive_exchange_rounds),
        ("hive+quiet", &totals.hive_exchange_quiet),
        ("hive+fact°", &totals.hive_aside_offfloor),
        ("hive+pooled", &totals.hive_pooled),
        ("hive+wide", &totals.hive_wide),
        ("hive+blind", &totals.hive_blind_wide),
    ]
    .iter()
    .enumerate()
    {
        let seed = mix(options.seed, 0xA51D_E000_u64.wrapping_add(index as u64));
        if let Some(line) = paired_against(name, "hive+", arm, &totals.hive_tuned, seed, 2000) {
            println!("{line}");
        }
    }
    if let Some(line) = paired_against(
        "hive+aside",
        "hive+ask",
        &totals.hive_aside,
        &totals.hive_ask,
        mix(options.seed, 0xA51D_E100),
        2000,
    ) {
        println!("{line}");
    }
}

/// Run every check-and-exchange arm over one room and fold each into
/// `totals`.
///
/// Split out of [`run_arms`] because there are now seven of them and they
/// form one experiment: three that vary who reads the answer and where the
/// question is aimed, one matched-turn control that throws the answer away,
/// one that holds the same exchange off the floor, and one free ceiling. Read
/// together they separate what an exchange is worth from what its turns cost;
/// read one at a time they do not.
///
/// # Errors
///
/// Returns the library's own error text from any arm.
fn run_check_arms(
    options: &Options,
    room: &Room,
    tuned: &EpisodePolicy,
    totals: &mut Totals,
) -> Result<(), String> {
    let check = |mode: AsideMode, style: CheckStyle| {
        run_episode_checking(room, tuned, TASK, false, mode, options.aside_cap, style)
    };
    // The pair that isolates privacy. Both spend a turn asking and a turn
    // answering; they differ in who may read the answer, and in nothing
    // else. `--aside-cap 0` leaves both bit-identical to `hive+`.
    totals
        .hive_aside
        .add(&check(AsideMode::Private, CheckStyle::PLAIN)?);
    totals
        .hive_ask
        .add(&check(AsideMode::Public, CheckStyle::PLAIN)?);
    // The informed variant: the check goes to whoever the room has heard
    // ground this option. It exists to close the obvious objection to a
    // negative result — that the question went to the wrong peer.
    totals
        .hive_aside_informed
        .add(&check(AsideMode::Private, CheckStyle::AIMED)?);
    // The same aimed exchange, carrying the fact rather than a number.
    // This is the arm that asks whether an aside is worth anything once
    // it carries what the room's public grammar has always carried.
    totals
        .hive_aside_fact
        .add(&check(AsideMode::Private, CheckStyle::FACT)?);
    // The matched-turn control the comparison always needed: the same
    // words on the same turns, with the answer thrown away. What it
    // loses against `hive+` is what the turns cost; what any arm above
    // gains over it is what the answer is worth.
    totals
        .hive_aside_mute
        .add(&check(AsideMode::Private, CheckStyle::MUTE)?);
    // The same aimed, fact-carrying exchange, riding alongside each member's
    // floor move instead of replacing one: one turn, two rows, the second of
    // which the episode cannot see. Same words and same targeting as
    // `hive+fact`; the room simply is not charged for it.
    totals
        .hive_aside_alongside
        .add(&check(AsideMode::Alongside, CheckStyle::ALONGSIDE)?);
    // The same free row, spent continuously: a contact on every turn carrying
    // every reading its author holds, bounded by how many distinct peers
    // `--aside-cap` allows. This is the arm a charged row could never afford.
    totals
        .hive_aside_exchange
        .add(&check(AsideMode::Alongside, CheckStyle::EXCHANGE)?);
    // `hive+share` saying nothing: the control for an *alongside* row, whose
    // sequence lands unevenly rather than once per turn.
    totals
        .hive_aside_hush
        .add(&check(AsideMode::Alongside, CheckStyle::QUIET)?);
    // The same continuous exchange, run off the floor: one round between every
    // pair of turns, bounded by `ExchangePolicy` rather than by the number of
    // turns the room takes. Priced in `calls/ep`.
    totals
        .hive_exchange_rounds
        .add(&run_episode_exchanging_with(
            room,
            tuned,
            TASK,
            false,
            options.exchange_cap,
            CheckStyle::EXCHANGE,
        )?);
    // The same rounds writing the same rows, with every answer discarded. The
    // difference between this and `hive+rounds` is what the exchange said; what
    // this arm moves on its own is what writing private rows does to a decay
    // that reads recency off raw sequence distance.
    totals.hive_exchange_quiet.add(&run_episode_exchanging_with(
        room,
        tuned,
        TASK,
        false,
        options.exchange_cap,
        CheckStyle::QUIET,
    )?);
    // The same bounded exchange, held off the floor entirely and given oracle
    // targeting. It bounds what the alongside arm above could reach.
    totals.hive_aside_offfloor.add(&run_episode(
        &room.pre_checked(options.aside_cap, true),
        tuned,
        TASK,
        false,
    )?);
    // `Room::pooled` is not gated by `cap` -- unlike the check arms above, it
    // has no notion of a bounded number of contacts. But `--aside-cap 0` is
    // documented and used as the kill switch that leaves every aside arm
    // bit-identical to `hive+`, `hive+pooled` included, so honor it here by
    // skipping the pool rather than silently pooling regardless of the cap.
    let ceiling = if options.aside_cap == 0 {
        room.clone()
    } else {
        room.pooled()
    };
    totals
        .hive_pooled
        .add(&run_episode(&ceiling, tuned, TASK, false)?);
    // The concurrency arm: the tuned policy, run in rounds rather than one
    // turn at a time. Same rooms, same budget, same everything else -- what
    // moves is `rounds/ep`, and whether `correct %` pays for it.
    totals.hive_wide.add(&run_episode(
        room,
        &widened_policy(tuned, options.round_width),
        TASK,
        false,
    )?);
    totals.hive_blind_wide.add(&run_episode(
        room,
        &blind_wide_policy(tuned, options.round_width),
        TASK,
        false,
    )?);
    Ok(())
}

/// Run every arm over the same rooms, and say how long the whole sample
/// took.
///
/// # Errors
///
/// Returns the library's own error text from any arm.
fn run_arms(options: &Options, rooms: &[Room]) -> Result<(Totals, std::time::Duration), String> {
    let tuned = options.policy;
    let default = default_policy();
    let wall = Instant::now();
    // Each room is decided into totals of its own, in a worker, and those are
    // merged here in room order. The body below is exactly the sequential
    // fold it replaced — what changed is who owns the `Totals` it folds into.
    // See `crate::parallel` for why the merge order is not a detail.
    // Indexed rather than bare, because two arms seed themselves off the
    // room's position in the sample and a worker cannot recover it from a
    // reference.
    let indexed: Vec<(usize, &Room)> = rooms.iter().enumerate().collect();
    let per_room: Vec<Totals> = parallel::map_in_order(&indexed, options.jobs, |(index, room)| {
        let (index, room) = (*index, *room);
        let mut totals = Totals::priced_at(options.cost_model);

        totals
            .hive_default
            .add(&run_episode(room, &default, TASK, false)?);
        totals
            .hive_tuned
            .add(&run_episode(room, &tuned, TASK, false)?);
        if options.cost {
            totals.all_reasoning.add(&run_episode(
                &room.at_cost(SPECIALIST_COST_UNIT),
                &tuned,
                TASK,
                false,
            )?);
        }
        run_check_arms(options, room, &tuned, &mut totals)?;
        let seed = mix(options.seed, u64::try_from(index).unwrap_or(0));
        totals.ladder.add_arm(&arms::run_ladder(room, seed)?);
        // The control is given the whole budget, which is more turns than the
        // deliberation actually spends. It is the arm to beat, so it gets
        // every advantage.
        totals
            .vote
            .add_arm(&arms::run_vote(room, tuned.turn_budget));
        Ok(totals)
    })?;

    let mut totals = Totals::priced_at(options.cost_model);
    for chunk in &per_room {
        totals.merge(chunk);
    }
    Ok((totals, wall.elapsed()))
}

/// Print how each deliberating arm's episodes ended.
fn endings(totals: &Totals) {
    println!();
    for (name, arm) in [
        ("hive ", &totals.hive_default),
        ("hive+", &totals.hive_tuned),
    ] {
        println!(
            "{name} endings: converged {} · deadlocked {} · exhausted {} · idle {}",
            arm.converged, arm.deadlocked, arm.exhausted, arm.idle,
        );
    }
}

/// Print what each arm spent, and what its right answers cost.
///
/// Only under `--cost-tiers`, where a specialist's turn is charged ten times
/// a lay member's and the question stops being "which arm is most accurate"
/// and becomes "which arm is most accurate per unit spent". `correct/kU` is
/// right answers per thousand cost units: an arm that buys two more points of
/// accuracy by putting every seat on the expensive tier should be visible
/// here as having bought them badly.
fn cost_table(arms: &[(&str, &Aggregate)]) {
    println!(
        "\n{:<15}{:>11}{:>10}{:>14}",
        "arm", "correct %", "cost/ep", "correct/kU",
    );
    for (name, totals) in arms {
        println!(
            "{:<15}{:>11.1}{:>10.2}{:>14.2}",
            name,
            totals.accuracy(),
            totals.cost_per_episode(),
            totals.accuracy_per_kilo_unit(),
        );
    }
}
