//! The broadcast router, the first-turn prompt and the system prompt.

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
use super::support::*;

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

#[tokio::test]
async fn each_recipient_of_concurrent_broadcasts_is_handed_its_own_senders_message() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    let pending = federation.ready().expect("ready");
    let at = federation.floors[0].journal.len();
    let broadcast = |to: &str, message: &str| super::super::tools::Action::Broadcast {
        to: to.into(),
        message: message.into(),
    };
    let results = vec![
        (
            "research-librarian".to_owned(),
            broadcast("research-scout", "from the librarian"),
        ),
        (
            "research-provenance".to_owned(),
            broadcast("research-librarian", "from provenance"),
        ),
        ("research-scout".to_owned(), complete("scout done")),
    ];
    assert_eq!(pending[0].seats.len(), 3);
    federation.commit(0, at, results).await.expect("commit");
    let scout = federation.assignment(0, "research-scout");
    let librarian = federation.assignment(0, "research-librarian");
    assert!(
        scout.note.starts_with("@research-librarian handed you:")
            && scout.note.ends_with("from the librarian"),
        "{}",
        scout.note
    );
    assert_eq!(
        librarian.note,
        "@research-provenance handed you: from provenance"
    );
}
