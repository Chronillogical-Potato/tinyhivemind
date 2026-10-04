//! Bids and salience: what makes a seat want the floor.

use tinyhivemind_core::hive::attention::BidContext;
use tinyhivemind_core::hive::quorum::{consensus, standings};
use tinyhivemind_core::hive::salience::importance;
use tinyhivemind_core::hive::trace::{TraceKind, read};
use tinyhivemind_core::hive::{
    AgentThreshold, Bid, EpisodePolicy, Horizon, QuorumPolicy, SalienceWeights, TopicId, bids,
    floor_holder, floor_round, salience,
};
use tinyhivemind_core::runtime::Sequence;
use tinyhivemind_lab::{Res, section};

use crate::market::journal;

fn show(list: &[Bid]) -> String {
    if list.is_empty() {
        return "(nobody bids)".into();
    }
    list.iter()
        .map(|b| format!("{}:{}/{:?}", b.agent_id, b.urge, b.reason))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn attention() {
    section("bids: dominance_cap, repetition_cap, weights and thresholds on one transcript");
    let rows = journal();
    let traces = read(&rows);
    let quorum = QuorumPolicy::DEFAULT;
    let table = standings(&traces, Sequence(7), &quorum).unwrap_or_default();
    let members = ["ada", "ben", "cy", "di", "eli"];
    let policy = EpisodePolicy::DEFAULT;
    let thresholds = [AgentThreshold::new("ada", 0)];
    let ask = |label: &str,
               dominance_cap: u32,
               repetition_cap: u32,
               weights: &SalienceWeights,
               thresholds: &[AgentThreshold]| {
        let context = BidContext {
            traces: &traces,
            standings: &table,
            members: &members,
            thresholds,
            at: Horizon::at(Sequence(7)),
            weights,
            dominance_cap,
            repetition_cap,
            quorum: &quorum,
        };
        match bids(&context) {
            Ok(list) => println!("  {label:<30} {}", show(&list)),
            Err(error) => println!("  {label:<30} refused: {error}"),
        }
    };
    let w = SalienceWeights::DEFAULT;
    ask(
        "defaults",
        policy.dominance_cap,
        policy.repetition_cap,
        &w,
        &thresholds,
    );
    ask(
        "dominance_cap=20 (ada dominates)",
        20,
        policy.repetition_cap,
        &w,
        &thresholds,
    );
    ask(
        "dominance_cap=100",
        100,
        policy.repetition_cap,
        &w,
        &thresholds,
    );
    ask("repetition_cap=1", policy.dominance_cap, 1, &w, &thresholds);
    ask(
        "repetition_cap=0 (off)",
        policy.dominance_cap,
        0,
        &w,
        &thresholds,
    );
    ask(
        "importance=0",
        policy.dominance_cap,
        policy.repetition_cap,
        &SalienceWeights { importance: 0, ..w },
        &thresholds,
    );
    ask(
        "half_life=1",
        policy.dominance_cap,
        policy.repetition_cap,
        &SalienceWeights { half_life: 1, ..w },
        &thresholds,
    );
    ask(
        "half_life=0",
        policy.dominance_cap,
        policy.repetition_cap,
        &SalienceWeights { half_life: 0, ..w },
        &thresholds,
    );
    let high = [
        AgentThreshold::new("ada", 100_000),
        AgentThreshold::new("ben", 100_000),
    ];
    ask(
        "ada and ben thresholds=100000",
        policy.dominance_cap,
        policy.repetition_cap,
        &w,
        &high,
    );
    let mut leaning = AgentThreshold::new("eli", 0);
    leaning.affinity = vec![(TopicId::from("x"), 100), (TopicId::from("y"), 0)];
    ask(
        "eli affinity x=100 y=0",
        policy.dominance_cap,
        policy.repetition_cap,
        &w,
        &[leaning],
    );
    ask(
        "duplicate thresholds",
        policy.dominance_cap,
        policy.repetition_cap,
        &w,
        &[AgentThreshold::new("ada", 0), AgentThreshold::new("ada", 1)],
    );

    let context = BidContext {
        traces: &traces,
        standings: &table,
        members: &members,
        thresholds: &thresholds,
        at: Horizon::at(Sequence(7)),
        weights: &w,
        dominance_cap: policy.dominance_cap,
        repetition_cap: policy.repetition_cap,
        quorum: &quorum,
    };
    if let Ok(list) = bids(&context) {
        println!(
            "  floor_holder -> {:?}",
            floor_holder(&list).map(|b| &b.agent_id)
        );
        for width in [1, 2, 3] {
            println!(
                "  floor_round(width {width}) -> {:?}",
                floor_round(&list, width)
                    .iter()
                    .map(|b| b.agent_id.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }
    println!(
        "  consensus of the standings -> {:?}",
        consensus(&table, &quorum)
    );
}

pub fn salience_table() -> Res {
    section("salience: one Support trace read at growing distances");
    let rows = journal();
    let traces = read(&rows);
    let Some(trace) = traces.iter().find(|t| t.kind == TraceKind::Support) else {
        return Ok(());
    };
    let live: Vec<Sequence> = rows.iter().map(|r| r.sequence).collect();
    println!(
        "  importance by kind: {}",
        [
            TraceKind::Commit,
            TraceKind::Propose,
            TraceKind::Refute,
            TraceKind::Object,
            TraceKind::Evidence,
            TraceKind::Support,
            TraceKind::Question,
            TraceKind::Defer
        ]
        .iter()
        .map(|k| format!("{k:?}={}", importance(*k)))
        .collect::<Vec<_>>()
        .join(" ")
    );
    for (label, weights) in [
        ("DEFAULT", SalienceWeights::DEFAULT),
        ("for_room(3)", SalienceWeights::for_room(3)),
        ("for_room(100)", SalienceWeights::for_room(100)),
        (
            "recency 50",
            SalienceWeights {
                recency: 50,
                ..SalienceWeights::DEFAULT
            },
        ),
        (
            "relevance 0",
            SalienceWeights {
                relevance: 0,
                ..SalienceWeights::DEFAULT
            },
        ),
    ] {
        let cells: Vec<String> = [3_u64, 10, 40, 200]
            .iter()
            .map(|at| {
                let value = salience(trace, Sequence(*at), &weights, 50)
                    .map(|s| s.0.to_string())
                    .unwrap_or_else(|e| e.to_string());
                format!("@{at}={value}")
            })
            .collect();
        println!("  {label:<14} {}", cells.join("  "));
    }
    let by_relevance: Vec<String> = [0_u8, 50, 100, 255]
        .iter()
        .map(|r| {
            format!(
                "{r}->{}",
                salience(trace, Sequence(10), &SalienceWeights::DEFAULT, *r)
                    .map(|s| s.0)
                    .unwrap_or(-1)
            )
        })
        .collect();
    println!(
        "  relevance 0/50/100/255 at @10 (saturates above 100): {}",
        by_relevance.join(" ")
    );
    let sparse = [Sequence(2), Sequence(40), Sequence(41)];
    let sequence = Horizon::at(Sequence(41));
    let live_h = Horizon::over(Sequence(41), &sparse);
    println!(
        "  Horizon: distance(^2) Sequence={} Live={}; within(^2, 5) Sequence={} Live={}",
        sequence.distance(Sequence(2)),
        live_h.distance(Sequence(2)),
        sequence.within(Sequence(2), 5),
        live_h.within(Sequence(2), 5)
    );
    println!(
        "  horizon.sequence() = {:?}; live rows of the journal: {live:?}",
        sequence.sequence()
    );
    println!(
        "  salience with half_life=0: {}",
        salience(
            trace,
            Sequence(10),
            &SalienceWeights {
                half_life: 0,
                ..SalienceWeights::DEFAULT
            },
            50
        )
        .err()
        .map_or("ok".into(), |e| e.to_string())
    );
    Ok(())
}
