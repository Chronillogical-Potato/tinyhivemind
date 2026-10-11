//! Package wiring: the problem placeholder, per-seat tool gate, tool schemas and the federation opening.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use tinyhivemind_core::embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingRequest, RoutingSource,
};

use super::super::agents::system_prompt;
use super::super::floors::{Assignment, Federation, Row};
use super::super::memory::{run_scoped, scope_binding};
use super::super::package::{self, SeatTool};
use super::super::router::addressed;
use super::super::sandbox::docker_args;
use super::super::search::{Provider, condense_exa};
use super::super::tools::{SeatScope, schema};
use super::super::turn::{TurnInput, prompt};


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
