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

#[test]
fn a_third_of_the_sandbox_budget_is_held_back_for_the_verify_desk() {
    let mut limits = super::budget::Limits::SMOKE;
    limits.max_sandbox_runs = 6;
    let budget = super::budget::Budget::new(limits);
    for _ in 0..4 {
        budget
            .claim_sandbox("compute")
            .expect("compute may use its share");
    }
    let refused = budget
        .claim_sandbox("compute")
        .expect_err("compute is out of its share");
    assert!(refused.to_string().contains("verify"), "{refused}");
    budget
        .claim_sandbox("verify")
        .expect("verify keeps its reserve");
    budget
        .claim_sandbox("verify")
        .expect("verify keeps its reserve");
    assert!(
        budget.claim_sandbox("verify").is_err(),
        "the total still binds"
    );
}

#[test]
fn a_verify_seat_may_support_only_after_running_a_check_this_turn() {
    use super::tools::{CallRecord, grounded};
    let support = Some(super::tools::Verdict::Support);
    assert!(grounded("verify", support, &[]).is_err());
    let failed = [CallRecord {
        tool: "run_python".into(),
        summary: String::new(),
        ok: false,
    }];
    assert!(grounded("verify", support, &failed).is_err());
    let ran = [CallRecord {
        tool: "run_python".into(),
        summary: String::new(),
        ok: true,
    }];
    assert!(grounded("verify", support, &ran).is_ok());
    assert!(grounded("verify", Some(super::tools::Verdict::Object), &[]).is_ok());
    assert!(grounded("compute", support, &[]).is_ok());
}

#[tokio::test]
async fn each_recipient_of_concurrent_broadcasts_is_handed_its_own_senders_message() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    let pending = federation.ready().expect("ready");
    let at = federation.floors[0].journal.len();
    let broadcast = |to: &str, message: &str| super::tools::Action::Broadcast {
        to: to.into(),
        message: message.into(),
    };
    let results = vec![
        ("research-librarian".to_owned(), broadcast("research-scout", "from the librarian")),
        ("research-provenance".to_owned(), broadcast("research-librarian", "from provenance")),
        ("research-scout".to_owned(), complete("scout done")),
    ];
    assert_eq!(pending[0].seats.len(), 3);
    federation.commit(0, at, results).await.expect("commit");
    let scout = federation.assignment(0, "research-scout");
    let librarian = federation.assignment(0, "research-librarian");
    assert!(
        scout.note.starts_with("@research-librarian handed you:") && scout.note.ends_with("from the librarian"),
        "{}",
        scout.note
    );
    assert_eq!(librarian.note, "@research-provenance handed you: from provenance");
}

fn memory_for(reads: Vec<tinyhivemind_lang::MemoryReach>) -> super::memory::SeatMemory {
    let (_, hive) = lowered();
    let mut binding = scope_binding(hive.seats[9].memory.as_ref().expect("bound"), "team:euler-hive", "r1")
        .expect("scoped");
    binding.settings.reads = reads;
    let engine: std::sync::Arc<dyn tinymemory_api::MemoryEngine> =
        std::sync::Arc::new(tinymemory_api::conformance::reference::ReferenceEngine::new());
    super::memory::SeatMemory::new(engine, binding, Some("team:euler-hive/ws:r1/project:verified".into()), true)
}

#[tokio::test]
async fn a_failed_recall_leaves_the_rejoin_watermark_where_it_was() {
    use tinyhivemind_core::runtime::{Recall, RecallMoment, RecallRequest};
    let broken = memory_for(vec![tinyhivemind_lang::MemoryReach {
        at: "not a namespace".into(),
        inherit: false,
        descendants: false,
    }]);
    let request = |moment| RecallRequest {
        seat: "verify-constraints".into(),
        conversation: "c".into(),
        focus: Some("findings".into()),
        moment,
        budget_chars: 500,
    };
    assert!(broken.recall(&request(RecallMoment::Rejoin)).await.is_err());
    assert!(broken.seen().is_none(), "a failed read must not advance the watermark");
    let working = memory_for(vec![]);
    working.recall(&request(RecallMoment::Rejoin)).await.expect("an empty read succeeds");
    assert!(working.seen().is_some());
}

#[tokio::test]
async fn nobody_publishes_to_a_read_only_pool() {
    let entry = tinyhivemind_core::runtime::RememberEntry {
        kind: tinyhivemind_core::runtime::EntryKind::Outcome,
        text: "checked".into(),
    };
    let writable = memory_for(vec![]);
    assert!(writable.pool_writer());
    writable.publish(&entry, "c").await.expect("a writable pool takes the finding");
    let pinned = memory_for(vec![]).read_only_pool(true);
    assert!(!pinned.pool_writer(), "the belt no longer offers verified=true");
    let refused = pinned.publish(&entry, "c").await.expect_err("a read-only pool refuses");
    assert!(refused.to_string().contains("read-only"));
}

#[test]
fn a_spend_cap_must_be_finite_and_non_negative() {
    assert_eq!(super::spend_cap("2.5").expect("a real cap"), 2.5);
    for bad in ["NaN", "inf", "-inf", "-1"] {
        assert!(super::spend_cap(bad).is_err(), "{bad} accepted");
    }
}

#[test]
fn a_problem_id_is_one_safe_path_component() {
    assert_eq!(super::path_component("pe1014").expect("plain id"), "pe1014");
    for bad in ["", "..", "../x", "/tmp/x", "a/b", "a b"] {
        assert!(super::path_component(bad).is_err(), "{bad:?} accepted");
    }
}

#[test]
fn artifact_names_cannot_leave_the_run_directory() {
    use super::artifacts::{contained, file_component};
    assert_eq!(file_component("verify-adversary"), "verify-adversary");
    assert_eq!(file_component("x/../../../../tmp/result"), "x_____________tmp_result");
    assert_eq!(file_component(""), "_");
    assert!(contained("turns/0001-seat.md").is_ok());
    for bad in ["../x", "/tmp/x", "turns/../../x"] {
        assert!(contained(bad).is_err(), "{bad} accepted");
    }
    let dir = tempfile::tempdir().expect("dir");
    let run = super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    run.write("turns/0001-seat.md", "ok");
    run.check().expect("a contained write lands");
    run.write("../escaped.md", "no");
    assert!(!dir.path().join("escaped.md").exists());
    assert!(run.check().is_err(), "a refused write is reported, not dropped");
}

#[test]
fn every_turn_artifact_gets_its_own_number() {
    let dir = tempfile::tempdir().expect("dir");
    let run = super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    let first = run.next_turn();
    let second = run.next_turn();
    assert_ne!(first, second);
}

#[test]
fn a_failed_frame_append_or_flush_is_reported() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("disk full"))
        }
    }
    let error = super::artifacts::append_to(&mut Broken, &serde_json::json!({"type": "end"}))
        .expect_err("a broken stream fails");
    assert!(error.to_string().contains("disk full"));
    let dir = tempfile::tempdir().expect("dir");
    let run = super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    run.note("frames.ndjson", Err(error));
    assert!(run.check().is_err());
}

#[test]
fn concurrent_claims_never_take_the_verify_reserve() {
    let mut limits = super::budget::Limits::SMOKE;
    limits.max_sandbox_runs = 30;
    let budget = std::sync::Arc::new(super::budget::Budget::new(limits));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let budget = budget.clone();
            std::thread::spawn(move || {
                (0..10)
                    .filter(|_| budget.claim_sandbox("compute").is_ok())
                    .count()
            })
        })
        .collect();
    let granted: usize = threads
        .into_iter()
        .map(|thread| thread.join().expect("claimant"))
        .sum();
    assert_eq!(granted, 20, "compute gets exactly its two-thirds share");
    for _ in 0..10 {
        budget.claim_sandbox("verify").expect("the reserve is intact");
    }
    assert!(budget.claim_sandbox("verify").is_err());
}

#[tokio::test]
async fn a_memory_call_past_the_wall_clock_is_abandoned() {
    use std::time::Duration;
    let pending = std::future::pending::<()>();
    assert!(super::turn::within(Duration::from_millis(20), pending).await.is_none());
    assert_eq!(super::turn::within(Duration::from_secs(5), async { 7 }).await, Some(7));
}

#[tokio::test]
async fn a_synthesis_report_without_a_candidate_is_no_decision() {
    let (_, hive) = lowered();
    let mut federation = Federation::new(&hive).expect("federation builds");
    federation.open("go");
    drive(&mut federation, |_, seat, _| complete(&format!("{seat} has no answer"))).await;
    assert!(federation.finished.is_none(), "nothing was decided");
    assert!(federation.undecided.is_some(), "synthesis reported without a candidate");
    assert_eq!(super::report::ending_of(false, true, None), "no_decision");
    assert_eq!(super::report::ending_of(true, false, None), "converged");
    assert_eq!(super::report::ending_of(false, false, Some("exhausted_turns")), "exhausted_turns");
}
