//! The responder ladder: exactly one agent answers one message.

use tinyhivemind_core::desk::{Desk, DeskSet, ResponderMode};
use tinyhivemind_core::mention::{Mention, MentionTarget};
use tinyhivemind_core::responder::{
    CandidateProbability, Probability, ResponderPlan, ResponderRequest, SelectionEvaluation,
    SelectionPolicy, SelectorCandidate, accept_evaluation, accept_selection, responder_plan,
};
use tinyhivemind_core::roster::{Roster, RosterMember};
use tinyhivemind_lab::{Res, section};

fn member(id: &str, name: &str) -> RosterMember {
    RosterMember {
        id: id.into(),
        name: Some(name.into()),
    }
}

fn desk(id: &str, name: &str, mode: ResponderMode, members: &[&str]) -> Desk {
    Desk {
        id: id.into(),
        name: name.into(),
        description: None,
        members: members.iter().map(|m| (*m).to_owned()).collect(),
        responder_mode: mode,
    }
}

fn p(parts: u32) -> Probability {
    Probability::new(parts).unwrap_or(Probability::ZERO)
}

fn request(
    chat: Option<&str>,
    mentions: Vec<Mention>,
    policy: SelectionPolicy,
) -> ResponderRequest {
    ResponderRequest {
        message: "who fixes the parser?".into(),
        chat: chat.map(str::to_owned),
        mentions,
        orchestrator_id: "boss".into(),
        selection_policy: policy,
        minimum_selection_confidence: p(600_000),
    }
}

fn mention(id: &str) -> Mention {
    Mention {
        target: MentionTarget::Agent { id: id.into() },
        text: format!("@{id}"),
        offset: 0,
        quiet: false,
    }
}

fn show(plan: &ResponderPlan) -> String {
    match plan {
        ResponderPlan::Decided { decision } => format!(
            "{} via {:?} ({:?})",
            decision.responder_id, decision.rung, decision.disposition
        ),
        ResponderPlan::Select { request, fallback } => format!(
            "SELECT among {:?} at >= {:.1}, fallback {} ({:?})",
            request
                .candidates
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            f64::from(request.minimum_confidence.parts()) / 1e6,
            fallback.responder_id,
            fallback.disposition
        ),
    }
}

pub fn run() -> Res {
    section("responder_plan: the ladder, rung by rung");
    let members = [
        member("boss", "Boss"),
        member("ada", "Ada"),
        member("ben", "Ben"),
        member("cy", "Cy"),
        member("old", "Old"),
    ];
    let retired = ["old".to_owned()];
    let roster = Roster::new(&members, &[], &retired);
    let desks = [
        desk("eng", "Engineering", ResponderMode::Lead, &["ada", "ben"]),
        desk("auto", "Autos", ResponderMode::Auto, &["ada", "ben", "cy"]),
        desk("solo", "Solo", ResponderMode::Auto, &["ada"]),
        desk("empty", "Empty", ResponderMode::Auto, &["old"]),
        desk("t1", "Twin", ResponderMode::Lead, &["ada"]),
        desk("t2", "Twin", ResponderMode::Lead, &["ben"]),
    ];
    let set = DeskSet::new(&desks, &[], &[], &[], &[]);
    let details = [SelectorCandidate {
        id: "ben".into(),
        label: "Ben".into(),
        role: "Parser owner".into(),
        description: Some("knows the grammar".into()),
    }];
    let allowed = SelectionPolicy::Allowed;
    let cases: Vec<(&str, ResponderRequest)> = vec![
        (
            "explicit mention wins",
            request(Some("eng"), vec![mention("cy")], allowed),
        ),
        (
            "a quiet or inactive mention does not",
            request(Some("eng"), vec![mention("old")], allowed),
        ),
        (
            "Lead desk -> first member",
            request(Some("eng"), vec![], allowed),
        ),
        (
            "Auto desk, selection allowed",
            request(Some("auto"), vec![], allowed),
        ),
        (
            "Auto desk, selection Disabled",
            request(Some("auto"), vec![], SelectionPolicy::Disabled),
        ),
        ("Auto desk of one", request(Some("solo"), vec![], allowed)),
        (
            "desk with no active member",
            request(Some("empty"), vec![], allowed),
        ),
        (
            "a desk addressed by name",
            request(Some("Engineering"), vec![], allowed),
        ),
        (
            "ambiguous desk name -> orchestrator",
            request(Some("Twin"), vec![], allowed),
        ),
        (
            "direct chat `dm:cy`",
            request(Some("dm:cy"), vec![], allowed),
        ),
        (
            "direct chat by display name",
            request(Some("Ben"), vec![], allowed),
        ),
        (
            "General -> orchestrator",
            request(Some("General"), vec![], allowed),
        ),
        ("no chat -> orchestrator", request(None, vec![], allowed)),
        (
            "unknown chat -> orchestrator",
            request(Some("nowhere"), vec![], allowed),
        ),
    ];
    for (label, req) in &cases {
        match responder_plan(req, &roster, &set, &details) {
            Ok(plan) => println!("  {label:<40} {}", show(&plan)),
            Err(error) => println!("  {label:<40} error: {error}"),
        }
    }
    let mut no_boss = request(Some("General"), vec![], allowed);
    no_boss.orchestrator_id = "ghost".into();
    println!(
        "  {:<40} {}",
        "orchestrator is not an active member",
        responder_plan(&no_boss, &roster, &set, &details)
            .err()
            .map_or("ok".into(), |e| e.to_string())
    );
    let twice = [details[0].clone(), details[0].clone()];
    println!(
        "  {:<40} {}",
        "duplicate selector details",
        responder_plan(
            &request(Some("auto"), vec![], allowed),
            &roster,
            &set,
            &twice
        )
        .err()
        .map_or("ok".into(), |e| e.to_string())
    );
    println!(
        "  Probability::new(1_000_001) = {:?}: an invalid minimum cannot be built, so InvalidProbability is unreachable from outside",
        Probability::new(1_000_001)
    );

    section("accepting a selector's answer");
    let candidates = [
        SelectorCandidate {
            id: "ada".into(),
            label: "Ada".into(),
            role: "r".into(),
            description: None,
        },
        SelectorCandidate {
            id: "ben".into(),
            label: "Ben".into(),
            role: "r".into(),
            description: None,
        },
    ];
    for output in [
        "ben",
        "  Ben.  ",
        "`ben`",
        "'BEN'",
        "\"ben\"",
        "ben or ada",
        "carol",
        "",
        "``",
    ] {
        println!(
            "  accept_selection({output:?}) = {:?}",
            accept_selection(output, &candidates)
        );
    }
    let eval = |choice: &str, ada: u32, ben: u32, confidence: u32| SelectionEvaluation {
        choice: choice.into(),
        probabilities: vec![
            CandidateProbability {
                candidate_id: "ada".into(),
                probability: p(ada),
            },
            CandidateProbability {
                candidate_id: "ben".into(),
                probability: p(ben),
            },
        ],
        confidence: p(confidence),
    };
    for (label, evaluation) in [
        (
            "confident choice of ben",
            eval("ben", 200_000, 800_000, 800_000),
        ),
        (
            "confidence below the minimum",
            eval("ben", 400_000, 600_000, 500_000),
        ),
        (
            "distribution does not sum to one",
            eval("ben", 200_000, 700_000, 800_000),
        ),
        (
            "chosen option is not the most likely",
            eval("ada", 200_000, 800_000, 800_000),
        ),
        (
            "choice outside the candidates",
            eval("zed", 200_000, 800_000, 800_000),
        ),
    ] {
        println!(
            "  accept_evaluation, {label:<36} = {:?}",
            accept_evaluation(&evaluation, &candidates, p(600_000))
        );
    }
    let mut short = eval("ben", 200_000, 800_000, 800_000);
    short.probabilities.pop();
    println!(
        "  accept_evaluation, a candidate missing from the table = {:?}",
        accept_evaluation(&short, &candidates, p(600_000))
    );
    Ok(())
}
