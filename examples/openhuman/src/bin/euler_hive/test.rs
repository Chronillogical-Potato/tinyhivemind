//! Offline tests for the Euler hive host: package wiring, the per-seat tool
//! gate, the sandbox's confinement, run-scoped memory, the prompt and the
//! broadcast router. No network, Docker, model or memory engine is touched.

use std::collections::BTreeSet;
use std::path::Path;

use tinyhivemind_core::embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingRequest, RoutingSource,
};

use super::agents::system_prompt;
use super::floors::{Assignment, Federation, Row};
use super::memory::{run_scoped, scope_binding};
use super::package::{self, SeatTool};
use super::router::addressed;
use super::sandbox::docker_args;
use super::search::{Provider, condense_exa};
use super::tools::{SeatScope, schema};
use super::turn::{TurnInput, prompt};

pub(super) fn package_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/tinyhivemind-lang/examples/hives/euler-hive")
}

fn lowered() -> (tinyhivemind_lang::Package, tinyhivemind_lang::LoweredHive) {
    let documents = package::read_package(&package_dir()).expect("package reads");
    package::load(documents, "Find the answer to a test problem.").expect("package lowers")
}

#[test]
fn the_host_injects_the_problem_into_the_placeholder() {
    let (package, hive) = lowered();
    let problem = &hive.context[package::PROBLEM_DOCUMENT];
    assert!(problem.contains("Find the answer to a test problem."));
    assert!(!problem.contains("placeholder"));
    assert_eq!(package.manifest.id, "euler-hive");
}

#[test]
fn a_package_without_the_problem_document_is_refused() {
    let mut documents = package::read_package(&package_dir()).expect("package reads");
    documents.remove(package::PROBLEM_DOCUMENT);
    assert!(package::load(documents, "x").is_err());
}

#[test]
fn problem_markdown_keeps_paragraphs_and_tex_and_drops_tags() {
    let html = "<p>A club has $7$ members&nbsp;named <b>A</b>.</p><p>Find &lt;it&gt;.</p>";
    assert_eq!(
        package::problem_markdown(html),
        "A club has $7$ members named A.\n\nFind <it>."
    );
}

#[test]
fn only_research_seats_get_web_search_and_only_compute_and_verify_get_python() {
    let (_, hive) = lowered();
    for seat in &hive.seats {
        let belt = package::seat_tools(seat).expect("every seat's belt resolves");
        let desk = package::desk_of(&hive, &seat.id).expect("seat has a desk");
        assert_eq!(
            belt.contains(&SeatTool::WebSearch),
            desk.id == "research",
            "{}",
            seat.id
        );
        assert_eq!(
            belt.contains(&SeatTool::RunPython),
            matches!(desk.id.as_str(), "compute" | "verify"),
            "{}",
            seat.id
        );
        for speaking in [SeatTool::Broadcast, SeatTool::Refer, SeatTool::Complete] {
            assert!(belt.contains(&speaking));
        }
        let names: BTreeSet<_> = belt.iter().map(|tool| tool.name()).collect();
        for harness in [
            "shell",
            "file_read",
            "file_write",
            "web_fetch",
            "http_request",
            "curl",
        ] {
            assert!(!names.contains(harness), "{} holds {harness}", seat.id);
        }
    }
}

#[test]
fn a_seat_allowing_an_unserved_or_denied_tool_is_refused() {
    let (_, hive) = lowered();
    let mut seat = hive.seats[0].clone();
    seat.tools.allow.push("shell".into());
    assert!(package::seat_tools(&seat).is_err());
    let mut seat = hive.seats[0].clone();
    seat.tools.deny.push("hive_complete".into());
    assert!(package::seat_tools(&seat).is_err());
    let mut seat = hive.seats[0].clone();
    seat.tools.allow.retain(|tool| tool != "hive_complete");
    assert!(package::seat_tools(&seat).is_err());
}

#[test]
fn the_sandbox_has_no_network_no_mounts_and_bounded_resources() {
    let args = docker_args("python:3.12-slim", "euler-test");
    let pair = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .map(|index| args[index + 1].as_str())
    };
    assert_eq!(pair("--network"), Some("none"));
    assert_eq!(pair("--memory"), Some("1g"));
    assert_eq!(pair("--pids-limit"), Some("128"));
    assert_eq!(pair("--cap-drop"), Some("ALL"));
    assert_eq!(pair("--security-opt"), Some("no-new-privileges"));
    assert_eq!(pair("--user"), Some("65534:65534"));
    assert!(args.contains(&"--read-only".to_owned()));
    assert!(args.contains(&"--rm".to_owned()));
    for mount in [
        "--mount",
        "-v",
        "--volume",
        "--volumes-from",
        "--privileged",
    ] {
        assert!(!args.iter().any(|arg| arg == mount), "{mount} present");
    }
    assert!(!args.iter().any(|arg| arg.contains("hive-runs")));
    let tail: Vec<_> = args.iter().rev().take(3).collect();
    assert_eq!(tail, ["-", "-I", "python3"]);
}

#[test]
fn memory_is_scoped_under_the_run_and_refuses_namespaces_outside_the_root() {
    assert_eq!(
        run_scoped("team:euler-hive/agent:x", "team:euler-hive", "r1").expect("scoped"),
        "team:euler-hive/ws:r1/agent:x"
    );
    assert_eq!(
        run_scoped("team:euler-hive", "team:euler-hive", "r1").expect("scoped"),
        "team:euler-hive/ws:r1"
    );
    assert!(run_scoped("team:other/agent:x", "team:euler-hive", "r1").is_err());
    let (_, hive) = lowered();
    for seat in &hive.seats {
        let binding = seat.memory.as_ref().expect("bound");
        let scoped = scope_binding(binding, "team:euler-hive", "r1").expect("scoped");
        assert_eq!(
            scoped.namespace,
            format!("team:euler-hive/ws:r1/agent:{}", seat.id)
        );
        assert_eq!(scoped.root, "team:euler-hive/ws:r1");
        assert!(
            scoped
                .settings
                .reads
                .iter()
                .all(|reach| reach.at.starts_with("team:euler-hive/ws:r1/"))
        );
        let harness = tinyhivemind_openhuman::language::memory_binding(&scoped)
            .expect("the adapter accepts a run-scoped agent binding");
        assert_eq!(harness.agent_id(), seat.id);
        assert_eq!(harness.root_namespace(), Some("team:euler-hive/ws:r1"));
    }
}

fn scope(desk: &str) -> SeatScope {
    SeatScope {
        seat: "s".into(),
        desk: desk.into(),
        colleagues: vec!["a".into(), "b".into()],
        linked_desks: vec!["verify".into()],
        conversation: "c".into(),
    }
}

#[test]
fn tool_schemas_offer_only_colleagues_linked_desks_and_served_providers() {
    let broadcast = schema(SeatTool::Broadcast, &scope("compute"), false);
    assert_eq!(
        broadcast["properties"]["to"]["enum"],
        serde_json::json!(["a", "b"])
    );
    let refer = schema(SeatTool::Refer, &scope("compute"), false);
    assert_eq!(
        refer["properties"]["desk"]["enum"],
        serde_json::json!(["verify"])
    );
    let search = schema(SeatTool::WebSearch, &scope("research"), false);
    assert_eq!(
        search["properties"]["provider"]["enum"],
        serde_json::json!(Provider::SEAT_CHOICES)
    );
    assert!(
        schema(SeatTool::Remember, &scope("theory"), false)["properties"]["verified"].is_null()
    );
    assert!(
        schema(SeatTool::Remember, &scope("verify"), true)["properties"]["verified"].is_object()
    );
    assert!(Provider::parse(Some("exa")).is_err());
    assert!(Provider::parse(None).is_ok());
}

#[test]
fn exa_results_condense_to_numbered_sources() {
    let found = condense_exa(&serde_json::json!({"results": [
        {"title": "Witt design", "url": "https://example.org/w", "text": "a design"},
        {"title": "No url"}
    ]}));
    assert!(
        found
            .text
            .starts_with("[1] Witt design\nhttps://example.org/w\na design")
    );
    assert_eq!(found.urls, ["https://example.org/w"]);
    assert_eq!(condense_exa(&serde_json::json!({})).text, "no results");
}

fn request(message: &str) -> RoutingRequest {
    let candidate = |id: &str| RouteCandidate {
        id: id.into(),
        label: id.into(),
        role: None,
        description: None,
        capabilities: vec![],
        learned_topics: vec![],
        available: true,
    };
    RoutingRequest {
        message: message.into(),
        source: RoutingSource::AgentBroadcast {
            author_id: "x".into(),
        },
        conversation: ConversationRef {
            id: "compute".into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: None,
        thread_context: vec![],
        candidates: vec![candidate("compute-linear"), candidate("compute-minimalist")],
        roster_version: 1,
        policy: lowered().1.policies.routing,
    }
}

#[test]
fn the_router_follows_the_named_colleague_and_nothing_else() {
    assert_eq!(
        addressed(&request("@compute-linear check this")),
        Some("compute-linear")
    );
    assert_eq!(addressed(&request("check this @compute-linear")), None);
    assert_eq!(addressed(&request("@nobody check")), None);
}

#[test]
fn the_federation_opens_on_the_first_desk_with_every_member_assigned() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    assert_eq!(
        federation.linked_from("verify"),
        ["synthesis", "compute", "theory"]
    );
    federation.open("go");
    let pending = federation.ready().expect("ready");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].desk, 0);
    assert!(pending[0].blind);
    assert_eq!(pending[0].seats, hive.desks[0].members);
    let assignment: Assignment = federation.assignment(0, &hive.desks[0].members[0]);
    assert_eq!(assignment.reason, "opening");
}

#[test]
fn the_first_turn_carries_the_problem_and_a_rejoin_does_not() {
    let input = TurnInput {
        problem: "THE PROBLEM".into(),
        desk_line: "#verify".into(),
        rows: vec![Row {
            sequence: 3,
            author: "compute-linear".into(),
            kind: "report",
            body: "candidate X".into(),
        }],
        assignment: Assignment {
            reason: "report".into(),
            note: "check X".into(),
        },
        budget_line: "5 turns".into(),
    };
    let first = prompt(&input, true, Some("## Hive memory\n- note"));
    assert!(first.starts_with("## The problem\nTHE PROBLEM"));
    assert!(first.contains("## Hive memory\n- note"));
    assert!(first.contains("[3] @compute-linear (report): candidate X"));
    let later = prompt(&input, false, None);
    assert!(!later.contains("THE PROBLEM"));
    assert!(later.ends_with("hive_complete, hive_broadcast or hive_refer."));
}

#[test]
fn the_system_prompt_opens_with_the_persona_and_carries_the_charter() {
    let text = system_prompt(
        "You are a \"tester\".\nDo it.",
        "Check things.",
        "## Protocol",
    );
    assert!(text.starts_with("You are a \"tester\"."));
    assert!(text.contains("## Your role\nCheck things."));
    assert!(text.ends_with("## Protocol"));
}

/// Drive a federation offline: `act` chooses each seat's action from its
/// desk and the reason it was called. Returns the rounds run.
async fn drive(
    federation: &mut Federation,
    act: impl Fn(&str, &str, &str) -> super::tools::Action,
) -> usize {
    let mut rounds = 0;
    while federation.finished.is_none() && rounds < 60 {
        let pending = federation.ready().expect("ready");
        if pending.is_empty() {
            break;
        }
        for round in pending {
            let desk = federation.floors[round.desk].desk.id.clone();
            let at = federation.floors[round.desk].journal.len();
            let results = round
                .seats
                .iter()
                .map(|seat| {
                    let reason = federation.assignment(round.desk, seat).reason;
                    (seat.clone(), act(&desk, seat, &reason))
                })
                .collect();
            federation
                .commit(round.desk, at, results)
                .await
                .expect("commit");
        }
        rounds += 1;
    }
    rounds
}

fn complete(report: &str) -> super::tools::Action {
    super::tools::Action::Complete {
        report: report.into(),
        candidate: None,
        verdict: None,
    }
}

fn refer(desk: &str) -> super::tools::Action {
    super::tools::Action::Refer {
        desk: desk.into(),
        message: "please check".into(),
    }
}

#[tokio::test]
async fn overlapping_questions_with_nested_asks_all_come_home_and_the_pipeline_finishes() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    // Two research seats ask theory at once; each theory answer first asks
    // compute, so two answering chains overlap on the theory desk.
    drive(&mut federation, |desk, seat, reason| match (desk, reason) {
        ("research", "opening") if seat != "research-scout" => refer("theory"),
        ("theory", "referral") => refer("compute"),
        _ => complete(&format!("{seat} done")),
    })
    .await;
    let research = &federation.floors[0];
    assert!(
        research.reported,
        "research never reported: a return was lost"
    );
    let finished = federation
        .finished
        .as_ref()
        .expect("the pipeline reaches synthesis");
    assert_eq!(finished.desk, "synthesis");
    let returns = federation
        .referrals
        .iter()
        .filter(|frame| frame.kind == "return" && frame.to_desk == "research" && frame.accepted)
        .count();
    assert_eq!(returns, 2, "both research questions were answered");
}

#[tokio::test]
async fn a_refused_ask_gives_the_seat_its_turn_back_before_the_desk_reports() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    // research may not refer to verify: no declared link.
    drive(&mut federation, |desk, seat, reason| {
        match (desk, seat, reason) {
            ("research", "research-librarian", "opening") => refer("verify"),
            _ => complete(&format!("{seat} done")),
        }
    })
    .await;
    let refused = federation
        .referrals
        .iter()
        .find(|frame| frame.kind == "forward")
        .expect("the ask is recorded");
    assert!(!refused.accepted);
    let research = &federation.floors[0];
    assert!(research.reported);
    assert!(
        research
            .journal
            .iter()
            .any(|row| row.kind == "complete" && row.author == "research-librarian"),
        "the refused seat completed after its turn came back"
    );
    assert!(federation.finished.is_some());
}

#[test]
fn searches_that_quote_the_statement_or_look_the_problem_up_are_refused() {
    use super::search::{refused_query, statement_literals};
    let statement = "Nights: ABCDKMPR and ABJKLMNW. Given S(10) = 1.1546112276 and n = 24.";
    let literals = statement_literals(statement);
    assert_eq!(literals, ["1.1546112276", "ABCDKMPR", "ABJKLMNW"]);
    assert!(refused_query("\"ABCDKMPR\" octad", &literals).is_some());
    assert!(refused_query("sum equal to 1.1546112276", &literals).is_some());
    assert!(refused_query("Project Euler rock paper scissors", &literals).is_some());
    assert!(refused_query("Witt design S(5,8,24) uniqueness", &literals).is_none());
    assert!(refused_query("Nash equilibrium skew-symmetric game support", &literals).is_none());
}
