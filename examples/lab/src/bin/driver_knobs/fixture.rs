//! The engineering desk every driver example runs on.

use tinyhivemind_core::desk::{Desk, ResponderMode};
use tinyhivemind_core::driver::{AgentBinding, BoundAgent, BoundHive, HiveGraph};
use tinyhivemind_core::embed::{RouteCandidate, RoutingPolicy};
use tinyhivemind_core::hive::CompletionEpisodeState;
use tinyhivemind_core::runtime::responder::Probability;
use tinyhivemind_core::runtime::{Conversation, Sequence};

/// The host's handle on a seat: here just the id of a runtime session.
#[derive(Clone, Debug)]
pub struct Runtime(pub String);

impl BoundAgent for Runtime {
    fn runtime_id(&self) -> &str {
        &self.0
    }
}

pub const SEATS: [&str; 4] = ["planner", "coder", "tester", "writer"];

pub fn candidates() -> Vec<RouteCandidate> {
    let table: [(&str, &[&str]); 4] = [
        ("planner", &["plan", "roadmap"]),
        ("coder", &["rust", "parser", "code"]),
        ("tester", &["tests", "qa", "edge"]),
        ("writer", &["docs", "guide"]),
    ];
    table
        .iter()
        .map(|(id, capabilities)| RouteCandidate {
            id: (*id).into(),
            label: (*id).into(),
            role: Some(format!("{id} seat")),
            description: None,
            capabilities: capabilities.iter().map(|c| (*c).to_owned()).collect(),
            learned_topics: Vec::new(),
            available: true,
        })
        .collect()
}

pub fn desk() -> Desk {
    Desk {
        id: "eng".into(),
        name: "Engineering".into(),
        description: Some("Build and ship the parser".into()),
        members: SEATS.iter().map(|s| (*s).to_owned()).collect(),
        responder_mode: ResponderMode::Auto,
    }
}

pub fn hive() -> BoundHive<Runtime> {
    let bindings = SEATS
        .iter()
        .map(|seat| AgentBinding::new(*seat, Runtime(format!("session-{seat}"))))
        .collect();
    BoundHive::new(HiveGraph::new(desk(), candidates()), bindings)
        .expect("the fixture hive is well formed")
}

pub fn routing_policy(round_width: usize) -> RoutingPolicy {
    let p = |parts: u32| Probability::new(parts).unwrap_or(Probability::ZERO);
    RoutingPolicy {
        minimum_confidence: p(500_000),
        high_impact_minimum_confidence: p(800_000),
        clarification_threshold: p(600_000),
        high_impact_threshold: p(700_000),
        round_width,
        choice_option_limit: 8,
    }
}

pub fn conversation() -> Conversation {
    Conversation {
        desk_id: "eng".into(),
        desk_name: "Engineering".into(),
        thread_root: None,
    }
}

/// An episode in which every seat holds an assignment made at `at`.
pub fn episode(at: u64) -> CompletionEpisodeState {
    CompletionEpisodeState::opened(conversation(), Sequence(at), SEATS)
        .expect("four distinct seats")
}
