//! Graph contract tests.

#![allow(clippy::expect_used)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tinyhivemind::desk::{Desk, ResponderMode};

use tinyhivemind_embed::{
    ConversationKind, MessageRoute, RouteCandidate, Router, RouterFuture, RoutingFallback,
    RoutingPlan, RoutingPolicy, RoutingSource,
};

use super::{AgentBinding, BoundHive, HiveGraph};
use crate::Error;
use crate::test_support::{Seat, fixture};

fn desk(members: &[&str]) -> Desk {
    Desk {
        id: "engineering".into(),
        name: "Engineering".into(),
        description: Some("ship reliable software".into()),
        members: members.iter().map(|id| (*id).into()).collect(),
        responder_mode: ResponderMode::Auto,
    }
}

fn candidate(id: &str) -> RouteCandidate {
    RouteCandidate {
        id: id.into(),
        label: id.into(),
        role: None,
        description: None,
        capabilities: Vec::new(),
        learned_topics: Vec::new(),
        available: true,
    }
}

fn bindings(ids: &[&str]) -> Vec<AgentBinding<Seat>> {
    ids.iter()
        .enumerate()
        .map(|(index, id)| AgentBinding::new(*id, fixture().agent(index)))
        .collect()
}

fn hive(ids: &[&str]) -> BoundHive<Seat> {
    BoundHive::new(
        HiveGraph::new(desk(ids), ids.iter().map(|id| candidate(id)).collect()),
        bindings(ids),
    )
    .expect("fixture hive validates")
}

#[test]
fn rejects_an_empty_desk() {
    let result = BoundHive::<Seat>::new(HiveGraph::new(desk(&[]), Vec::new()), Vec::new());
    assert!(matches!(result, Err(Error::EmptyMembership { .. })));
}

#[test]
fn rejects_duplicate_and_mismatched_candidate_ids_before_bindings() {
    let graph = HiveGraph::new(
        desk(&["one", "two"]),
        vec![candidate("one"), candidate("one")],
    );
    let result = BoundHive::<Seat>::new(graph, Vec::new());
    assert!(matches!(
        result,
        Err(Error::DuplicateCandidateId { ref agent_id }) if agent_id == "one"
    ));
}

#[test]
fn rejects_duplicate_member_and_binding_ids() {
    let duplicate_member = BoundHive::new(
        HiveGraph::new(
            desk(&["one", "one"]),
            vec![candidate("one"), candidate("one")],
        ),
        vec![
            AgentBinding::new("one", fixture().agent(0)),
            AgentBinding::new("one", fixture().agent(0)),
        ],
    );
    assert!(matches!(
        duplicate_member,
        Err(Error::DuplicateMemberId { ref agent_id }) if agent_id == "one"
    ));

    let duplicate_binding = BoundHive::new(
        HiveGraph::new(
            desk(&["one", "two"]),
            vec![candidate("one"), candidate("two")],
        ),
        vec![
            AgentBinding::new("one", fixture().agent(0)),
            AgentBinding::new("one", fixture().agent(1)),
        ],
    );
    assert!(matches!(
        duplicate_binding,
        Err(Error::DuplicateBindingId { ref agent_id }) if agent_id == "one"
    ));
}

#[test]
fn requires_candidate_and_binding_ids_to_exactly_equal_membership() {
    let missing_candidate = BoundHive::new(
        HiveGraph::new(desk(&["one", "two"]), vec![candidate("one")]),
        bindings(&["one", "two"]),
    );
    assert!(matches!(
        missing_candidate,
        Err(Error::MembershipMismatch {
            collection: "candidates",
            ref missing,
            ref outsiders,
        }) if missing == &["two"] && outsiders.is_empty()
    ));

    let outsider_binding = BoundHive::new(
        HiveGraph::new(
            desk(&["one", "two"]),
            vec![candidate("one"), candidate("two")],
        ),
        vec![
            AgentBinding::new("one", fixture().agent(0)),
            AgentBinding::new("outsider", fixture().agent(1)),
        ],
    );
    assert!(matches!(
        outsider_binding,
        Err(Error::MembershipMismatch {
            collection: "bindings",
            ref missing,
            ref outsiders,
        }) if missing == &["two"] && outsiders == &["outsider"]
    ));
}

#[test]
fn rejects_blank_and_reserved_ids_in_every_canonical_collection() {
    for invalid in ["", "   "] {
        let member = BoundHive::new(
            HiveGraph::new(desk(&[invalid]), vec![candidate(invalid)]),
            vec![AgentBinding::new(invalid, fixture().agent(0))],
        );
        assert!(matches!(
            member,
            Err(Error::BlankCanonicalId {
                collection: "members"
            })
        ));

        let route_candidate = BoundHive::new(
            HiveGraph::new(desk(&["one"]), vec![candidate(invalid)]),
            bindings(&["one"]),
        );
        assert!(matches!(
            route_candidate,
            Err(Error::BlankCanonicalId {
                collection: "candidates"
            })
        ));

        let binding = BoundHive::new(
            HiveGraph::new(desk(&["one"]), vec![candidate("one")]),
            vec![AgentBinding::new(invalid, fixture().agent(0))],
        );
        assert!(matches!(
            binding,
            Err(Error::BlankCanonicalId {
                collection: "bindings"
            })
        ));
    }

    for (collection, result) in [
        (
            "members",
            BoundHive::new(
                HiveGraph::new(desk(&["none"]), vec![candidate("none")]),
                vec![AgentBinding::new("none", fixture().agent(0))],
            ),
        ),
        (
            "candidates",
            BoundHive::new(
                HiveGraph::new(desk(&["one"]), vec![candidate("none")]),
                bindings(&["one"]),
            ),
        ),
        (
            "bindings",
            BoundHive::new(
                HiveGraph::new(desk(&["one"]), vec![candidate("one")]),
                vec![AgentBinding::new("none", fixture().agent(0))],
            ),
        ),
    ] {
        assert!(matches!(
            result,
            Err(Error::ReservedCanonicalId { collection: held }) if held == collection
        ));
    }
}

#[test]
fn canonical_ids_are_independent_of_runtime_ids_and_handles_can_span_hives() {
    let shared = fixture().agent(0);
    let first = BoundHive::new(
        HiveGraph::new(desk(&["planner"]), vec![candidate("planner")]),
        vec![AgentBinding::new("planner", shared.clone())],
    )
    .expect("runtime id mismatch is permitted");
    let second = BoundHive::new(
        HiveGraph::new(desk(&["reviewer"]), vec![candidate("reviewer")]),
        vec![AgentBinding::new("reviewer", shared)],
    )
    .expect("a cloned handle may join another hive");

    assert_eq!(
        first
            .binding("planner")
            .expect("binding")
            .runtime_agent_id(),
        "runtime-one"
    );
    assert_eq!(
        second
            .binding("reviewer")
            .expect("binding")
            .runtime_agent_id(),
        "runtime-one"
    );
}

#[test]
fn resolves_routes_and_keeps_dms_private_to_the_hive() {
    let hive = hive(&["one", "two"]);
    let plan = RoutingPlan::Fallback {
        responder_id: "two".into(),
        reason: RoutingFallback::ExplicitMention,
    };
    let resolved = hive.resolve_plan(&plan).expect("member route resolves");
    assert_eq!(resolved[0].hive_agent_id, "two");

    let route = hive
        .resolve_dm("one", &["two".into()], 1)
        .expect("member DM resolves");
    assert_eq!(
        route,
        MessageRoute::desk_aside(vec!["two".into()], 1).expect("bounded aside")
    );
    assert!(matches!(
        hive.resolve_dm("one", &["outsider".into()], 1),
        Err(Error::UnknownDmRecipient { .. })
    ));
    assert!(matches!(
        hive.resolve_dm("outsider", &["two".into()], 1),
        Err(Error::UnknownDmSender { .. })
    ));
}

#[test]
fn rejects_invalid_private_recipient_sets() {
    let hive = hive(&["one", "two"]);
    assert!(matches!(
        hive.resolve_dm("one", &[], 1),
        Err(Error::EmptyDmRecipients)
    ));
    assert!(matches!(
        hive.resolve_dm("one", &["two".into(), "two".into()], 2),
        Err(Error::DuplicateDmRecipient { .. })
    ));
    assert!(matches!(
        hive.resolve_dm("one", &["two".into()], 0),
        Err(Error::DmTooWide { .. })
    ));
}

#[derive(Debug)]
struct CountingRouter(Arc<AtomicUsize>);

impl Router for CountingRouter {
    fn evaluate<'a>(
        &'a self,
        _request: &'a tinyhivemind_embed::RoutingRequest,
    ) -> RouterFuture<'a> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err("router must not be called".into()) })
    }
}

fn policy() -> RoutingPolicy {
    use tinyhivemind::responder::Probability;

    let probability = Probability::new(500_000).expect("bounded probability");
    RoutingPolicy {
        minimum_confidence: probability,
        high_impact_minimum_confidence: probability,
        clarification_threshold: probability,
        high_impact_threshold: probability,
        round_width: 1,
        choice_option_limit: 3,
    }
}

#[test]
fn route_desk_rejects_requests_outside_the_hive_graph_without_router_calls() {
    let hive = hive(&["one", "two"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let router = CountingRouter(Arc::clone(&calls));

    let mut cases = Vec::new();
    let mut wrong_id = hive.desk_request("help", vec![], None, 1, policy());
    wrong_id.conversation.id = "another-desk".into();
    cases.push(wrong_id);
    let mut wrong_kind = hive.desk_request("help", vec![], None, 1, policy());
    wrong_kind.conversation.kind = ConversationKind::Direct;
    cases.push(wrong_kind);
    let mut wrong_source = hive.desk_request("help", vec![], None, 1, policy());
    wrong_source.source = RoutingSource::AgentBroadcast {
        author_id: "one".into(),
    };
    cases.push(wrong_source);
    let mut wrong_candidates = hive.desk_request("help", vec![], None, 1, policy());
    wrong_candidates.candidates.swap(0, 1);
    cases.push(wrong_candidates);
    let mut wrong_availability = hive.desk_request("help", vec![], None, 1, policy());
    wrong_availability.candidates[0].available = false;
    cases.push(wrong_availability);

    for request in cases {
        assert!(matches!(
            fixture().block_on(hive.route_desk(Some(&router), None, &request, None, "one")),
            Err(Error::MismatchedDeskRoutingRequest { .. })
        ));
    }
    let request = hive.desk_request("help", vec![], None, 1, policy());
    for (explicit, fallback) in [(Some("outsider"), "one"), (None, "outsider")] {
        assert!(matches!(
            fixture().block_on(hive.route_desk(Some(&router), None, &request, explicit, fallback)),
            Err(Error::UnknownBoundAgent { .. })
        ));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// ── a handle of the host's own type ─────────────────────────────────────────

/// A host's own seat: the driver stores it and hands it back, and never
/// needs it to be a runtime agent.
#[derive(Clone, Debug)]
struct HostSeat {
    runtime_id: String,
}

impl super::BoundAgent for HostSeat {
    fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
}

#[test]
fn a_hive_binds_any_handle_that_names_its_runtime_id() {
    let graph = super::HiveGraph::new(
        tinyhivemind::desk::Desk {
            id: "engineering".into(),
            name: "Engineering".into(),
            description: None,
            members: vec!["lead".into(), "solver".into()],
            responder_mode: tinyhivemind::desk::ResponderMode::Auto,
        },
        ["lead", "solver"]
            .into_iter()
            .map(|id| tinyhivemind_embed::RouteCandidate {
                id: id.into(),
                label: id.into(),
                role: None,
                description: None,
                capabilities: Vec::new(),
                learned_topics: Vec::new(),
                available: true,
            })
            .collect(),
    );
    let hive = super::BoundHive::new(
        graph,
        ["lead", "solver"]
            .into_iter()
            .map(|id| {
                super::AgentBinding::new(
                    id,
                    HostSeat {
                        runtime_id: format!("{id}-raw"),
                    },
                )
            })
            .collect(),
    )
    .expect("members, candidates and bindings agree");
    assert_eq!(
        hive.binding("lead")
            .map(super::AgentBinding::runtime_agent_id),
        Some("lead-raw")
    );
    let episode = tinyhivemind_hive::CompletionEpisodeState::opened(
        tinyhivemind::Conversation {
            desk_id: "engineering".into(),
            desk_name: "Engineering".into(),
            thread_root: None,
        },
        tinyhivemind::Sequence(0),
        ["lead", "solver"],
    )
    .expect("opens");
    let driver = crate::CompletionDriver::new(&hive, 2).expect("width");
    let state = driver.start(episode).expect("starts");
    let round = driver.pending_round(&state).expect("a round");
    let handed_back: Vec<&str> = round
        .agents()
        .iter()
        .map(|pending| pending.agent.runtime_id.as_str())
        .collect();
    assert_eq!(handed_back, vec!["lead-raw", "solver-raw"]);
}
