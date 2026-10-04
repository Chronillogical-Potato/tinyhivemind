//! The mechanisms under the episode: bids, salience, the directory, division,
//! exchange rounds, and evaluated quorum.

use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::hive::attention::BidContext;
use tinyhivemind_core::hive::quorum::{consensus, standings, standings_with_evaluations};
use tinyhivemind_core::hive::salience::importance;
use tinyhivemind_core::hive::trace::{TRACE_CAP, TraceKind, read, resolve};
use tinyhivemind_core::hive::{
    AdmissionPolicy, AgentThreshold, Bid, DecisionEvaluation, DirectoryPolicy, DivisionPolicy,
    EpisodePolicy, EpisodeState, ExchangePolicy, ExchangeRound, ExchangeState, Horizon,
    QuorumPolicy, SalienceWeights, TopicId, TopicProbability, bids, directory, divide,
    exchange, floor_holder, floor_round, salience, step_with_evaluations,
};
use tinyhivemind_core::runtime::responder::Probability;
use tinyhivemind_core::runtime::{Conversation, Sequence, SessionAuthor, SessionMessage};
use tinyhivemind_lab::{Res, TraceRig, World, agent, row, section};

use crate::room::world;

fn desk(seq: u64, who: &str, body: &str) -> SessionMessage {
    row(seq, agent(who), body, Audience::Desk)
}

fn aside(seq: u64, who: &str, to: &str, body: &str) -> SessionMessage {
    row(seq, agent(who), body, Audience::Aside { members: vec![to.into()] })
}

fn journal() -> Vec<SessionMessage> {
    vec![
        row(1, SessionAuthor::Operator, "Pick a plan.", Audience::Desk),
        desk(2, "ada", "!propose #x plan x ^1"),
        desk(3, "ben", "!support #x agree ^2"),
        desk(4, "cy", "!propose #y plan y ^1"),
        desk(5, "ada", "!evidence #x the benchmark holds ^2"),
        desk(6, "di", "!object >2 ^2 that plan is weak"),
        desk(7, "ada", "!support #x again ^2"),
    ]
}

fn show(list: &[Bid]) -> String {
    if list.is_empty() {
        return "(nobody bids)".into();
    }
    list.iter().map(|b| format!("{}:{}/{:?}", b.agent_id, b.urge, b.reason)).collect::<Vec<_>>().join(" ")
}

pub fn run(rig: &TraceRig) -> Res {
    attention();
    salience_table()?;
    directory_and_division()?;
    exchange_rounds()?;
    evaluated_quorum(rig)?;
    trace_grammar();
    Ok(())
}

fn attention() {
    section("bids: dominance_cap, repetition_cap, weights and thresholds on one transcript");
    let rows = journal();
    let traces = read(&rows);
    let quorum = QuorumPolicy::DEFAULT;
    let table = standings(&traces, Sequence(7), &quorum).unwrap_or_default();
    let members = ["ada", "ben", "cy", "di", "eli"];
    let policy = EpisodePolicy::DEFAULT;
    let thresholds = [AgentThreshold::new("ada", 0)];
    let ask = |label: &str, dominance_cap: u32, repetition_cap: u32, weights: &SalienceWeights, thresholds: &[AgentThreshold]| {
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
    ask("defaults", policy.dominance_cap, policy.repetition_cap, &w, &thresholds);
    ask("dominance_cap=20 (ada dominates)", 20, policy.repetition_cap, &w, &thresholds);
    ask("dominance_cap=100", 100, policy.repetition_cap, &w, &thresholds);
    ask("repetition_cap=1", policy.dominance_cap, 1, &w, &thresholds);
    ask("repetition_cap=0 (off)", policy.dominance_cap, 0, &w, &thresholds);
    ask("importance=0", policy.dominance_cap, policy.repetition_cap, &SalienceWeights { importance: 0, ..w }, &thresholds);
    ask("half_life=1", policy.dominance_cap, policy.repetition_cap, &SalienceWeights { half_life: 1, ..w }, &thresholds);
    ask("half_life=0", policy.dominance_cap, policy.repetition_cap, &SalienceWeights { half_life: 0, ..w }, &thresholds);
    let high = [AgentThreshold::new("ada", 100_000), AgentThreshold::new("ben", 100_000)];
    ask("ada and ben thresholds=100000", policy.dominance_cap, policy.repetition_cap, &w, &high);
    let mut leaning = AgentThreshold::new("eli", 0);
    leaning.affinity = vec![(TopicId::from("x"), 100), (TopicId::from("y"), 0)];
    ask("eli affinity x=100 y=0", policy.dominance_cap, policy.repetition_cap, &w, &[leaning]);
    ask("duplicate thresholds", policy.dominance_cap, policy.repetition_cap, &w, &[AgentThreshold::new("ada", 0), AgentThreshold::new("ada", 1)]);

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
        println!("  floor_holder -> {:?}", floor_holder(&list).map(|b| &b.agent_id));
        for width in [1, 2, 3] {
            println!("  floor_round(width {width}) -> {:?}", floor_round(&list, width).iter().map(|b| b.agent_id.as_str()).collect::<Vec<_>>());
        }
    }
    println!("  consensus of the standings -> {:?}", consensus(&table, &quorum));
}

fn salience_table() -> Res {
    section("salience: one Support trace read at growing distances");
    let rows = journal();
    let traces = read(&rows);
    let Some(trace) = traces.iter().find(|t| t.kind == TraceKind::Support) else {
        return Ok(());
    };
    let live: Vec<Sequence> = rows.iter().map(|r| r.sequence).collect();
    println!("  importance by kind: {}", [TraceKind::Commit, TraceKind::Propose, TraceKind::Refute, TraceKind::Object, TraceKind::Evidence, TraceKind::Support, TraceKind::Question, TraceKind::Defer].iter().map(|k| format!("{k:?}={}", importance(*k))).collect::<Vec<_>>().join(" "));
    for (label, weights) in [
        ("DEFAULT", SalienceWeights::DEFAULT),
        ("for_room(3)", SalienceWeights::for_room(3)),
        ("for_room(100)", SalienceWeights::for_room(100)),
        ("recency 50", SalienceWeights { recency: 50, ..SalienceWeights::DEFAULT }),
        ("relevance 0", SalienceWeights { relevance: 0, ..SalienceWeights::DEFAULT }),
    ] {
        let cells: Vec<String> = [3_u64, 10, 40, 200]
            .iter()
            .map(|at| {
                let value = salience(trace, Sequence(*at), &weights, 50).map(|s| s.0.to_string()).unwrap_or_else(|e| e.to_string());
                format!("@{at}={value}")
            })
            .collect();
        println!("  {label:<14} {}", cells.join("  "));
    }
    let by_relevance: Vec<String> = [0_u8, 50, 100, 255].iter().map(|r| format!("{r}->{}", salience(trace, Sequence(10), &SalienceWeights::DEFAULT, *r).map(|s| s.0).unwrap_or(-1))).collect();
    println!("  relevance 0/50/100/255 at @10 (saturates above 100): {}", by_relevance.join(" "));
    let sparse = [Sequence(2), Sequence(40), Sequence(41)];
    let sequence = Horizon::at(Sequence(41));
    let live_h = Horizon::over(Sequence(41), &sparse);
    println!(
        "  Horizon: distance(^2) Sequence={} Live={}; within(^2, 5) Sequence={} Live={}",
        sequence.distance(Sequence(2)), live_h.distance(Sequence(2)),
        sequence.within(Sequence(2), 5), live_h.within(Sequence(2), 5)
    );
    println!("  horizon.sequence() = {:?}; live rows of the journal: {live:?}", sequence.sequence());
    println!(
        "  salience with half_life=0: {}",
        salience(trace, Sequence(10), &SalienceWeights { half_life: 0, ..SalienceWeights::DEFAULT }, 50).err().map_or("ok".into(), |e| e.to_string())
    );
    Ok(())
}

fn directory_and_division() -> Res {
    section("directory + division: who knows what, and who takes each facet");
    let rows = vec![
        row(1, SessionAuthor::Operator, "Ship the migration.", Audience::Desk),
        desk(2, "ben", "!evidence #db the schema has 3 tables ^1"),
        desk(3, "ada", "!propose #api stage it ^2"),
        desk(4, "cy", "!evidence #api the gateway retries ^1"),
        desk(5, "ada", "!support #db ^2 ben is right"),
        desk(6, "di", "!defer #ui not mine"),
        desk(7, "ada", "!evidence #ui mock is ready ^1"),
        desk(8, "ben", "!question #ui who owns the copy?"),
    ];
    let traces = read(&rows);
    let mut priors = vec![AgentThreshold::new("di", 0)];
    priors[0].affinity = vec![(TopicId::from("ui"), 90)];
    let policies = [
        ("DEFAULT", DirectoryPolicy::DEFAULT),
        ("specialisation only", DirectoryPolicy { credibility: 0, prior: 0, ..DirectoryPolicy::DEFAULT }),
        ("credibility only", DirectoryPolicy { specialisation: 0, prior: 0, ..DirectoryPolicy::DEFAULT }),
        ("prior only", DirectoryPolicy { specialisation: 0, credibility: 0, ..DirectoryPolicy::DEFAULT }),
        ("window=3", DirectoryPolicy { window: 3, ..DirectoryPolicy::DEFAULT }),
        ("half_life=2", DirectoryPolicy { half_life: 2, ..DirectoryPolicy::DEFAULT }),
        ("discredit=0", DirectoryPolicy { discredit: 0, ..DirectoryPolicy::DEFAULT }),
        ("floor=900000", DirectoryPolicy { floor: 900_000, ..DirectoryPolicy::DEFAULT }),
    ];
    let mut kept = None;
    for (label, policy) in policies {
        let dir = directory(&traces, Sequence(8), &policy, &priors)?;
        let entries: Vec<String> = dir.entries().iter().map(|e| format!("{}#{}={}", e.agent_id, e.topic, e.weight)).collect();
        println!("  {label:<22} {}", entries.join(" "));
        if label == "DEFAULT" {
            println!(
                "    topics {:?}; top(#db)={:?}; knows(ben,#db)={}; knows(ada,#ui)={}; top_among(#api,[ada,cy])={:?}",
                dir.topics().iter().map(|t| t.as_str()).collect::<Vec<_>>(),
                dir.top(&TopicId::from("db")).map(|e| &e.agent_id),
                dir.knows("ben", &TopicId::from("db"), &policy),
                dir.knows("ada", &TopicId::from("ui"), &policy),
                dir.top_among(&TopicId::from("api"), &["ada", "cy"])
            );
            kept = Some(dir);
        }
    }
    println!(
        "  directory with half_life=0: {}; with window=0: {}",
        directory(&traces, Sequence(8), &DirectoryPolicy { half_life: 0, ..DirectoryPolicy::DEFAULT }, &[]).err().map_or("ok".into(), |e| e.to_string()),
        directory(&traces, Sequence(8), &DirectoryPolicy { window: 0, ..DirectoryPolicy::DEFAULT }, &[]).err().map_or("ok".into(), |e| e.to_string()),
    );

    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let facets: Vec<TopicId> = ["db", "api", "ui", "ui", "docs", "ops"].iter().map(|f| TopicId::from(*f)).collect();
    for (label, known, policy) in [
        ("no directory, width 4", None, DivisionPolicy::DEFAULT),
        ("directory, width 4", kept.as_ref(), DivisionPolicy::DEFAULT),
        ("directory, width 1", kept.as_ref(), DivisionPolicy { round_width: 1, ..DivisionPolicy::DEFAULT }),
        ("directory ignored", kept.as_ref(), DivisionPolicy { follow_directory: false, ..DivisionPolicy::DEFAULT }),
    ] {
        let division = divide(&facets, "war", &roster, &desks, known, policy)?;
        let rounds: Vec<String> = (0..division.depth())
            .map(|r| format!("[{}]", division.round(r).iter().map(|a| format!("{}->{}{}", a.facet, a.owner, if a.reason == tinyhivemind_core::hive::OwnerReason::Knows { "*" } else { "" })).collect::<Vec<_>>().join(" ")))
            .collect();
        println!("  {label:<22} depth={} width={} {}", division.depth(), division.width(), rounds.join(" "));
    }
    let division = divide(&facets, "war", &roster, &desks, kept.as_ref(), DivisionPolicy::DEFAULT)?;
    println!(
        "  (* = OwnerReason::Knows) owner_of(#api)={:?} facets_of(ada)={:?} is_alone={} is_empty={}",
        division.owner_of(&TopicId::from("api")),
        division.facets_of("ada").iter().map(|t| t.as_str()).collect::<Vec<_>>(),
        division.is_alone(),
        division.is_empty()
    );
    let ui = TopicId::from("ui");
    let scoped = division.scoped(&ui, &rows);
    println!("  scoped(#ui) hands the owner {} of {} rows; the other facets' rows stay with their owners", scoped.len(), rows.len());
    println!("  single facet is_alone: {}", divide(&[TopicId::from("db")], "war", &roster, &desks, None, DivisionPolicy::DEFAULT)?.is_alone());
    println!("  no facets: is_empty={}", divide(&[], "war", &roster, &desks, None, DivisionPolicy::DEFAULT)?.is_empty());
    println!("  round_width=0: {}", divide(&facets, "war", &roster, &desks, None, DivisionPolicy { round_width: 0, ..DivisionPolicy::DEFAULT }).err().map_or("ok".into(), |e| e.to_string()));
    let empty = World::new().agent("zed").desk("void", "Void", "nobody", &["zed"]).retire("zed");
    println!("  a desk whose only member is retired: {}", divide(&facets, "void", &empty.roster(), &empty.desks(), None, DivisionPolicy::DEFAULT).err().map_or("ok".into(), |e| e.to_string()));
    Ok(())
}

fn exchange_rounds() -> Res {
    section("exchange: off-floor private rounds that spend model calls, not turns");
    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let state = EpisodeState::opened(Conversation { desk_id: "war".into(), desk_name: "War Room".into(), thread_root: None }, Sequence(1));
    let mut transcript = journal();
    for (label, policy) in [
        ("DEFAULT (off)", ExchangePolicy::DEFAULT),
        ("on, contact_cap=2, round_cap=3", ExchangePolicy { enabled: true, contact_cap: 2, round_cap: 3 }),
        ("on, contact_cap=1, round_cap=9", ExchangePolicy { enabled: true, contact_cap: 1, round_cap: 9 }),
        ("on, contact_cap=0", ExchangePolicy { enabled: true, contact_cap: 0, round_cap: 3 }),
    ] {
        let mut opened = ExchangeState::opened();
        let mut log = Vec::new();
        for round in 0..6 {
            match exchange(&policy, &state, opened, &transcript, &roster, &desks)? {
                ExchangeRound::Open { members, remaining, next } => {
                    log.push(format!("r{round}: open {} seat(s), {remaining} row(s) still reachable", members.len()));
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
    let solo = World::new().agent("ada").desk("war", "War Room", "alone", &["ada"]);
    let closed = exchange(&ExchangePolicy { enabled: true, contact_cap: 2, round_cap: 3 }, &state, ExchangeState::opened(), &[], &solo.roster(), &solo.desks())?;
    println!("  one seat on the desk: {closed:?}");
    println!("  ExchangeState::opened().advanced().rounds = {}", ExchangeState::opened().advanced().rounds);
    Ok(())
}

fn evaluated_quorum(rig: &TraceRig) -> Res {
    section("quorum by evaluation: probabilities instead of head counts");
    let tracer = rig.tracer("swarm:evaluated");
    let rows = journal();
    let traces = read(&rows);
    let at = Sequence(7);
    let quorum = QuorumPolicy::DEFAULT;
    let p = |parts: u32| Probability::new(parts).unwrap_or(Probability::ZERO);
    let eval = |agent: &str, source: u64, x: u32, evidence: u32, violation: u32| DecisionEvaluation {
        source_sequence: Sequence(source),
        agent_id: agent.into(),
        stance: vec![
            TopicProbability { topic: Some(TopicId::from("x")), probability: p(x) },
            TopicProbability { topic: Some(TopicId::from("y")), probability: p(1_000_000 - x) },
        ],
        evidence_quality: p(evidence),
        violation_probability: p(violation),
    };
    let relaxed = AdmissionPolicy { maximum_violation_probability: p(300_000) };
    let strict = AdmissionPolicy { maximum_violation_probability: p(50_000) };
    let full = [eval("ada", 7, 900_000, 1_000_000, 10_000), eval("ben", 3, 900_000, 1_000_000, 10_000)];
    let weak = [eval("ada", 7, 900_000, 400_000, 10_000), eval("ben", 3, 900_000, 400_000, 10_000)];
    let risky = [eval("ada", 7, 900_000, 1_000_000, 200_000), eval("ben", 3, 900_000, 1_000_000, 10_000)];
    let none: [DecisionEvaluation; 0] = [];
    let table = |label: &str, evals: &[DecisionEvaluation], admission: &AdmissionPolicy| {
        match standings_with_evaluations(&traces, evals, at, &quorum, admission) {
            Ok(list) => {
                let cells: Vec<String> = list.iter().map(|s| format!("#{} {:.2}/{} carried={}", s.topic, s.probability_support as f64 / 1e6, quorum.threshold, s.carried(&quorum))).collect();
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
    table("stale source (ada at ^2, not ^7)", &[eval("ada", 2, 900_000, 1_000_000, 0)], &relaxed);
    table("stance sums to 0.9", &[DecisionEvaluation { stance: vec![TopicProbability { topic: Some(TopicId::from("x")), probability: p(900_000) }], ..eval("ada", 7, 0, 1_000_000, 0) }], &relaxed);
    println!("  Probability::new(1_000_001) = {:?} (out of range is not constructible)", Probability::new(1_000_001));

    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let state = EpisodeState::opened(Conversation { desk_id: "war".into(), desk_name: "War Room".into(), thread_root: None }, Sequence(1));
    let policy = EpisodePolicy::DEFAULT;
    for (label, evals) in [("with evaluations", &full[..]), ("with none", &none[..])] {
        let result = step_with_evaluations(&state, &rows, &roster, &desks, &policy, evals, &relaxed)?;
        tracer.step(&result);
        let kind = match &result {
            tinyhivemind_core::hive::HiveStep::Speak { turns, .. } => format!("Speak({} seat(s), phase {:?})", turns.len(), turns.first().map(|t| t.phase)),
            other => format!("{other:?}").chars().take(60).collect(),
        };
        println!("  step_with_evaluations {label:<17} -> {kind}");
    }
    Ok(())
}

fn trace_grammar() {
    section("trace grammar: resolve, revalidation, TRACE_CAP");
    let body = "!propose #x plan ^1\n```\n!propose #fenced inside code\n```\n!refute #x\n!defer\n!support #x ^2 ^2";
    let found = resolve(body, None, &agent("ada"), Sequence(9));
    for t in &found {
        println!("  {:?} topic={:?} cites={:?} target={:?} grounded={}", t.kind, t.topic.as_ref().map(|x| x.as_str()), t.cites, t.target, t.grounded());
    }
    println!("  (the fenced propose, the ungrounded !refute and the topicless !defer yield no trace)");
    let supplied = resolve(body, Some(found.clone()), &agent("ada"), Sequence(9));
    let forged = resolve("plain text", Some(found), &agent("ada"), Sequence(9));
    println!("  supplied traces revalidated against the body: {} kept; against a body with none: {}", supplied.len(), forged.len());
    let flood: String = (0..TRACE_CAP + 8).map(|n| format!("!support #t{n} ^1\n")).collect();
    println!("  {} markers in one message -> {} traces (TRACE_CAP={TRACE_CAP})", TRACE_CAP + 8, resolve(&flood, None, &agent("ada"), Sequence(9)).len());
}
