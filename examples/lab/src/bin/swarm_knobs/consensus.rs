//! Off-floor exchange, evaluated quorum, and the trace grammar.

use tinyhivemind_core::hive::quorum::standings_with_evaluations;
use tinyhivemind_core::hive::trace::{TRACE_CAP, read, resolve};
use tinyhivemind_core::hive::{
    AdmissionPolicy, DecisionEvaluation, EpisodePolicy, EpisodeState, ExchangePolicy,
    ExchangeRound, ExchangeState, QuorumPolicy, TopicId, TopicProbability, exchange,
    step_with_evaluations,
};
use tinyhivemind_core::runtime::responder::Probability;
use tinyhivemind_core::runtime::{Conversation, Sequence};
use tinyhivemind_lab::{Res, TraceRig, World, agent, section};

use crate::market::{aside, desk, journal};
use crate::room::world;

pub fn exchange_rounds() -> Res {
    section("exchange: off-floor private rounds that spend model calls, not turns");
    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let state = EpisodeState::opened(
        Conversation {
            desk_id: "war".into(),
            desk_name: "War Room".into(),
            thread_root: None,
        },
        Sequence(1),
    );
    let mut transcript = journal();
    for (label, policy) in [
        ("DEFAULT (off)", ExchangePolicy::DEFAULT),
        (
            "on, contact_cap=2, round_cap=3",
            ExchangePolicy {
                enabled: true,
                contact_cap: 2,
                round_cap: 3,
            },
        ),
        (
            "on, contact_cap=1, round_cap=9",
            ExchangePolicy {
                enabled: true,
                contact_cap: 1,
                round_cap: 9,
            },
        ),
        (
            "on, contact_cap=0",
            ExchangePolicy {
                enabled: true,
                contact_cap: 0,
                round_cap: 3,
            },
        ),
    ] {
        let mut opened = ExchangeState::opened();
        let mut log = Vec::new();
        for round in 0..6 {
            match exchange(&policy, &state, opened, &transcript, &roster, &desks)? {
                ExchangeRound::Open {
                    members,
                    remaining,
                    next,
                } => {
                    log.push(format!(
                        "r{round}: open {} seat(s), {remaining} row(s) still reachable",
                        members.len()
                    ));
                    // Each eligible member appends one private row.
                    for member in &members {
                        let seq = transcript.len() as u64 + 1;
                        transcript.push(aside(seq, member, "ada", "a private word"));
                    }
                    opened = next;
                }
                ExchangeRound::Closed { reason } => {
                    log.push(format!("r{round}: closed {reason:?}"));
                    break;
                }
            }
        }
        println!("  {label:<32} {}", log.join(" | "));
        transcript = journal();
    }
    let solo = World::new()
        .agent("ada")
        .desk("war", "War Room", "alone", &["ada"]);
    let closed = exchange(
        &ExchangePolicy {
            enabled: true,
            contact_cap: 2,
            round_cap: 3,
        },
        &state,
        ExchangeState::opened(),
        &[],
        &solo.roster(),
        &solo.desks(),
    )?;
    println!("  one seat on the desk: {closed:?}");
    println!(
        "  ExchangeState::opened().advanced().rounds = {}",
        ExchangeState::opened().advanced().rounds
    );
    Ok(())
}

pub fn evaluated_quorum(rig: &TraceRig) -> Res {
    section("quorum by evaluation: probabilities instead of head counts");
    let tracer = rig.tracer("swarm:evaluated");
    let rows = journal();
    let traces = read(&rows);
    let at = Sequence(7);
    let quorum = QuorumPolicy::DEFAULT;
    let p = |parts: u32| Probability::new(parts).unwrap_or(Probability::ZERO);
    let eval =
        |agent: &str, source: u64, x: u32, evidence: u32, violation: u32| DecisionEvaluation {
            source_sequence: Sequence(source),
            agent_id: agent.into(),
            stance: vec![
                TopicProbability {
                    topic: Some(TopicId::from("x")),
                    probability: p(x),
                },
                TopicProbability {
                    topic: Some(TopicId::from("y")),
                    probability: p(1_000_000 - x),
                },
            ],
            evidence_quality: p(evidence),
            violation_probability: p(violation),
        };
    let relaxed = AdmissionPolicy {
        maximum_violation_probability: p(300_000),
    };
    let strict = AdmissionPolicy {
        maximum_violation_probability: p(50_000),
    };
    let full = [
        eval("ada", 7, 900_000, 1_000_000, 10_000),
        eval("ben", 3, 900_000, 1_000_000, 10_000),
    ];
    let weak = [
        eval("ada", 7, 900_000, 400_000, 10_000),
        eval("ben", 3, 900_000, 400_000, 10_000),
    ];
    let risky = [
        eval("ada", 7, 900_000, 1_000_000, 200_000),
        eval("ben", 3, 900_000, 1_000_000, 10_000),
    ];
    let none: [DecisionEvaluation; 0] = [];
    let table = |label: &str, evals: &[DecisionEvaluation], admission: &AdmissionPolicy| {
        match standings_with_evaluations(&traces, evals, at, &quorum, admission) {
            Ok(list) => {
                let cells: Vec<String> = list
                    .iter()
                    .map(|s| {
                        format!(
                            "#{} {:.2}/{} carried={}",
                            s.topic,
                            s.probability_support as f64 / 1e6,
                            quorum.threshold,
                            s.carried(&quorum)
                        )
                    })
                    .collect();
                println!("  {label:<34} {}", cells.join("  "));
            }
            Err(error) => println!("  {label:<34} refused: {error}"),
        }
    };
    table("head count (no evaluations asked)", &none, &relaxed);
    table("both confident, relaxed admission", &full, &relaxed);
    table("evidence_quality 0.4", &weak, &relaxed);
    table("ada violation 0.2, relaxed (<=0.3)", &risky, &relaxed);
    table("ada violation 0.2, strict (<=0.05)", &risky, &strict);
    table(
        "stale source (ada at ^2, not ^7)",
        &[eval("ada", 2, 900_000, 1_000_000, 0)],
        &relaxed,
    );
    table(
        "stance sums to 0.9",
        &[DecisionEvaluation {
            stance: vec![TopicProbability {
                topic: Some(TopicId::from("x")),
                probability: p(900_000),
            }],
            ..eval("ada", 7, 0, 1_000_000, 0)
        }],
        &relaxed,
    );
    println!(
        "  Probability::new(1_000_001) = {:?} (out of range is not constructible)",
        Probability::new(1_000_001)
    );

    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let state = EpisodeState::opened(
        Conversation {
            desk_id: "war".into(),
            desk_name: "War Room".into(),
            thread_root: None,
        },
        Sequence(1),
    );
    let policy = EpisodePolicy::DEFAULT;
    for (label, evals) in [("with evaluations", &full[..]), ("with none", &none[..])] {
        let result =
            step_with_evaluations(&state, &rows, &roster, &desks, &policy, evals, &relaxed)?;
        tracer.step(&result);
        let kind = match &result {
            tinyhivemind_core::hive::HiveStep::Speak { turns, .. } => format!(
                "Speak({} seat(s), phase {:?})",
                turns.len(),
                turns.first().map(|t| t.phase)
            ),
            other => format!("{other:?}").chars().take(60).collect(),
        };
        println!("  step_with_evaluations {label:<17} -> {kind}");
    }
    Ok(())
}

pub fn trace_grammar() {
    section("trace grammar: resolve, revalidation, TRACE_CAP");
    let body = "!propose #x plan ^1\n```\n!propose #fenced inside code\n```\n!refute #x\n!defer\n!support #x ^2 ^2";
    let found = resolve(body, None, &agent("ada"), Sequence(9));
    for t in &found {
        println!(
            "  {:?} topic={:?} cites={:?} target={:?} grounded={}",
            t.kind,
            t.topic.as_ref().map(|x| x.as_str()),
            t.cites,
            t.target,
            t.grounded()
        );
    }
    println!(
        "  (the fenced propose, the ungrounded !refute and the topicless !defer yield no trace)"
    );
    let supplied = resolve(body, Some(found.clone()), &agent("ada"), Sequence(9));
    let forged = resolve("plain text", Some(found), &agent("ada"), Sequence(9));
    println!(
        "  supplied traces revalidated against the body: {} kept; against a body with none: {}",
        supplied.len(),
        forged.len()
    );
    let flood: String = (0..TRACE_CAP + 8)
        .map(|n| format!("!support #t{n} ^1\n"))
        .collect();
    println!(
        "  {} markers in one message -> {} traces (TRACE_CAP={TRACE_CAP})",
        TRACE_CAP + 8,
        resolve(&flood, None, &agent("ada"), Sequence(9)).len()
    );
}
