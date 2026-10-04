//! The directory of who knows what, and the division of facets it informs.

use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::hive::trace::read;
use tinyhivemind_core::hive::{
    AgentThreshold, DirectoryPolicy, DivisionPolicy, TopicId, directory, divide,
};
use tinyhivemind_core::runtime::{Sequence, SessionAuthor};
use tinyhivemind_lab::{Res, World, row, section};

use crate::market::desk;
use crate::room::world;

pub fn directory_and_division() -> Res {
    section("directory + division: who knows what, and who takes each facet");
    let rows = vec![
        row(
            1,
            SessionAuthor::Operator,
            "Ship the migration.",
            Audience::Desk,
        ),
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
        (
            "specialisation only",
            DirectoryPolicy {
                credibility: 0,
                prior: 0,
                ..DirectoryPolicy::DEFAULT
            },
        ),
        (
            "credibility only",
            DirectoryPolicy {
                specialisation: 0,
                prior: 0,
                ..DirectoryPolicy::DEFAULT
            },
        ),
        (
            "prior only",
            DirectoryPolicy {
                specialisation: 0,
                credibility: 0,
                ..DirectoryPolicy::DEFAULT
            },
        ),
        (
            "window=3",
            DirectoryPolicy {
                window: 3,
                ..DirectoryPolicy::DEFAULT
            },
        ),
        (
            "half_life=2",
            DirectoryPolicy {
                half_life: 2,
                ..DirectoryPolicy::DEFAULT
            },
        ),
        (
            "discredit=0",
            DirectoryPolicy {
                discredit: 0,
                ..DirectoryPolicy::DEFAULT
            },
        ),
        (
            "floor=900000",
            DirectoryPolicy {
                floor: 900_000,
                ..DirectoryPolicy::DEFAULT
            },
        ),
    ];
    let mut kept = None;
    for (label, policy) in policies {
        let dir = directory(&traces, Sequence(8), &policy, &priors)?;
        let entries: Vec<String> = dir
            .entries()
            .iter()
            .map(|e| format!("{}#{}={}", e.agent_id, e.topic, e.weight))
            .collect();
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
        directory(
            &traces,
            Sequence(8),
            &DirectoryPolicy {
                half_life: 0,
                ..DirectoryPolicy::DEFAULT
            },
            &[]
        )
        .err()
        .map_or("ok".into(), |e| e.to_string()),
        directory(
            &traces,
            Sequence(8),
            &DirectoryPolicy {
                window: 0,
                ..DirectoryPolicy::DEFAULT
            },
            &[]
        )
        .err()
        .map_or("ok".into(), |e| e.to_string()),
    );

    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    let facets: Vec<TopicId> = ["db", "api", "ui", "ui", "docs", "ops"]
        .iter()
        .map(|f| TopicId::from(*f))
        .collect();
    for (label, known, policy) in [
        ("no directory, width 4", None, DivisionPolicy::DEFAULT),
        ("directory, width 4", kept.as_ref(), DivisionPolicy::DEFAULT),
        (
            "directory, width 1",
            kept.as_ref(),
            DivisionPolicy {
                round_width: 1,
                ..DivisionPolicy::DEFAULT
            },
        ),
        (
            "directory ignored",
            kept.as_ref(),
            DivisionPolicy {
                follow_directory: false,
                ..DivisionPolicy::DEFAULT
            },
        ),
    ] {
        let division = divide(&facets, "war", &roster, &desks, known, policy)?;
        let rounds: Vec<String> = (0..division.depth())
            .map(|r| {
                format!(
                    "[{}]",
                    division
                        .round(r)
                        .iter()
                        .map(|a| format!(
                            "{}->{}{}",
                            a.facet,
                            a.owner,
                            if a.reason == tinyhivemind_core::hive::OwnerReason::Knows {
                                "*"
                            } else {
                                ""
                            }
                        ))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            })
            .collect();
        println!(
            "  {label:<22} depth={} width={} {}",
            division.depth(),
            division.width(),
            rounds.join(" ")
        );
    }
    let division = divide(
        &facets,
        "war",
        &roster,
        &desks,
        kept.as_ref(),
        DivisionPolicy::DEFAULT,
    )?;
    println!(
        "  (* = OwnerReason::Knows) owner_of(#api)={:?} facets_of(ada)={:?} is_alone={} is_empty={}",
        division.owner_of(&TopicId::from("api")),
        division
            .facets_of("ada")
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>(),
        division.is_alone(),
        division.is_empty()
    );
    let ui = TopicId::from("ui");
    let scoped = division.scoped(&ui, &rows);
    println!(
        "  scoped(#ui) hands the owner {} of {} rows; the other facets' rows stay with their owners",
        scoped.len(),
        rows.len()
    );
    println!(
        "  single facet is_alone: {}",
        divide(
            &[TopicId::from("db")],
            "war",
            &roster,
            &desks,
            None,
            DivisionPolicy::DEFAULT
        )?
        .is_alone()
    );
    println!(
        "  no facets: is_empty={}",
        divide(&[], "war", &roster, &desks, None, DivisionPolicy::DEFAULT)?.is_empty()
    );
    println!(
        "  round_width=0: {}",
        divide(
            &facets,
            "war",
            &roster,
            &desks,
            None,
            DivisionPolicy {
                round_width: 0,
                ..DivisionPolicy::DEFAULT
            }
        )
        .err()
        .map_or("ok".into(), |e| e.to_string())
    );
    let empty = World::new()
        .agent("zed")
        .desk("void", "Void", "nobody", &["zed"])
        .retire("zed");
    println!(
        "  a desk whose only member is retired: {}",
        divide(
            &facets,
            "void",
            &empty.roster(),
            &empty.desks(),
            None,
            DivisionPolicy::DEFAULT
        )
        .err()
        .map_or("ok".into(), |e| e.to_string())
    );
    Ok(())
}

