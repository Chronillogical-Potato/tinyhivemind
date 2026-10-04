//! One knob at a time: rounds, turns and outcome for each `EpisodePolicy`.

use tinyhivemind_core::hive::{
    AgentThreshold, Basis, DEFAULT_REVEALED_WIDTH, DEFAULT_ROUND_WIDTH, EpisodePolicy,
    QuorumPolicy, SalienceWeights,
};
use tinyhivemind_lab::{Res, TraceRig, section};

use crate::room::{Room, episode};

type Tweak = Box<dyn Fn(&mut EpisodePolicy, &mut Room)>;

struct Case {
    group: &'static str,
    label: String,
    tweak: Tweak,
}

fn case(
    group: &'static str,
    label: impl Into<String>,
    tweak: impl Fn(&mut EpisodePolicy, &mut Room) + 'static,
) -> Case {
    Case {
        group,
        label: label.into(),
        tweak: Box::new(tweak),
    }
}

fn cases() -> Vec<Case> {
    let mut all = vec![case("baseline", "EpisodePolicy::DEFAULT", |_, _| {})];
    for budget in [3, 6, 12] {
        all.push(case(
            "turn_budget",
            format!("turn_budget={budget}"),
            move |p, _| p.turn_budget = budget,
        ));
    }
    for width in [1, 2, 4, 5] {
        all.push(case(
            "round_width",
            format!("round_width={width}"),
            move |p, _| p.round_width = width,
        ));
    }
    for width in [1, 2, 3] {
        all.push(case(
            "revealed_width",
            format!("revealed_width={width}"),
            move |p, _| p.revealed_width = width,
        ));
    }
    for blind in [true, false] {
        all.push(case(
            "blind_round",
            format!("blind_round={blind}"),
            move |p, _| p.blind_round = blind,
        ));
    }
    for cap in [20, 50, 100] {
        all.push(case(
            "dominance_cap",
            format!("dominance_cap={cap}"),
            move |p, _| p.dominance_cap = cap,
        ));
    }
    for cap in [1, 3, 10] {
        all.push(case(
            "repetition_cap",
            format!("repetition_cap={cap}"),
            move |p, _| p.repetition_cap = cap,
        ));
    }
    for (basis, label) in [(Basis::Sequence, "Sequence"), (Basis::Live, "Live")] {
        all.push(case(
            "distance",
            format!("{label}, window=9, 3 private rows per turn"),
            move |p, r| {
                p.distance = basis;
                p.quorum.window = 9;
                r.chatter = 3;
            },
        ));
    }
    for threshold in [1, 2, 3, 4] {
        all.push(case(
            "quorum.threshold",
            format!("threshold={threshold}"),
            move |p, _| p.quorum.threshold = threshold,
        ));
    }
    for window in [3, 6, 30] {
        all.push(case(
            "quorum.window",
            format!("window={window}"),
            move |p, _| p.quorum.window = window,
        ));
    }
    for grounded in [true, false] {
        all.push(case(
            "quorum.require_grounded",
            format!("require_grounded={grounded}, seats cite nothing"),
            move |p, r| {
                p.quorum.require_grounded = grounded;
                r.sloppy = true;
            },
        ));
    }
    all.push(case("for_room", "EpisodePolicy::for_room(5)", |p, _| {
        *p = EpisodePolicy::for_room(5)
    }));
    for (label, weights) in [
        (
            "recency=0",
            SalienceWeights {
                recency: 0,
                ..SalienceWeights::DEFAULT
            },
        ),
        (
            "recency=60",
            SalienceWeights {
                recency: 60,
                ..SalienceWeights::DEFAULT
            },
        ),
        (
            "importance=0",
            SalienceWeights {
                importance: 0,
                ..SalienceWeights::DEFAULT
            },
        ),
        (
            "relevance=0",
            SalienceWeights {
                relevance: 0,
                ..SalienceWeights::DEFAULT
            },
        ),
        (
            "half_life=2",
            SalienceWeights {
                half_life: 2,
                ..SalienceWeights::DEFAULT
            },
        ),
        (
            "half_life=200",
            SalienceWeights {
                half_life: 200,
                ..SalienceWeights::DEFAULT
            },
        ),
        ("for_room(40)", SalienceWeights::for_room(40)),
    ] {
        all.push(case("weights", label, move |p, _| p.weights = weights));
    }
    let mut eager = AgentThreshold::new("eli", -50_000);
    eager.affinity = vec![("z".into(), 100)];
    let shy = AgentThreshold::new("ada", 50_000);
    all.push(case(
        "thresholds",
        "eli eager (-50k, affinity z=100), ada shy (+50k)",
        move |_, r| {
            r.thresholds = vec![eager.clone(), shy.clone()];
        },
    ));
    all
}

/// Policies the fold refuses outright, with the reason it gives.
fn refused() -> Vec<(&'static str, EpisodePolicy)> {
    vec![
        (
            "round_width=0",
            EpisodePolicy {
                round_width: 0,
                ..EpisodePolicy::DEFAULT
            },
        ),
        (
            "revealed_width=0",
            EpisodePolicy {
                revealed_width: 0,
                ..EpisodePolicy::DEFAULT
            },
        ),
        (
            "quorum.threshold=0",
            EpisodePolicy {
                quorum: QuorumPolicy {
                    threshold: 0,
                    ..QuorumPolicy::DEFAULT
                },
                ..EpisodePolicy::DEFAULT
            },
        ),
        (
            "quorum.window=0",
            EpisodePolicy {
                quorum: QuorumPolicy {
                    window: 0,
                    ..QuorumPolicy::DEFAULT
                },
                ..EpisodePolicy::DEFAULT
            },
        ),
        (
            "weights.half_life=0",
            EpisodePolicy {
                weights: SalienceWeights {
                    half_life: 0,
                    ..SalienceWeights::DEFAULT
                },
                ..EpisodePolicy::DEFAULT
            },
        ),
    ]
}

pub fn run(rig: &TraceRig) -> Res {
    section("EpisodePolicy sweep: one knob at a time, five scripted seats");
    println!(
        "defaults: round_width={DEFAULT_ROUND_WIDTH} revealed_width={DEFAULT_REVEALED_WIDTH} {:?}",
        EpisodePolicy::DEFAULT
    );
    println!(
        "\n{:<24} {:<54} {:>6} {:>5} {:>6}  {:<18} first speakers",
        "knob", "setting", "rounds", "turns", "widest", "outcome"
    );
    let mut group = "";
    for case in cases() {
        let mut policy = EpisodePolicy::DEFAULT;
        let mut room = Room::default();
        (case.tweak)(&mut policy, &mut room);
        let tracer = rig.tracer(&format!("swarm:{}:{}", case.group, case.label));
        let shown = if case.group == group { "" } else { case.group };
        group = case.group;
        match episode(&room, &policy, &tracer) {
            Ok(o) => println!(
                "{shown:<24} {:<54} {:>6} {:>5} {:>6}  {:<18} {}",
                case.label,
                o.rounds,
                o.turns,
                o.widest,
                o.end,
                o.order.join(",")
            ),
            Err(error) => println!("{shown:<24} {:<54} refused: {error}", case.label),
        }
    }
    println!("\nrefused policies:");
    for (label, policy) in refused() {
        let tracer = rig.tracer(&format!("swarm:refused:{label}"));
        match episode(&Room::default(), &policy, &tracer) {
            Ok(o) => println!("  {label:<22} ran: {}", o.end),
            Err(error) => println!("  {label:<22} -> {error}"),
        }
    }
    Ok(())
}
