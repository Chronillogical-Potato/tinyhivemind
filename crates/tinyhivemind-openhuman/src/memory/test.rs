//! Root and memory agent id derivation and validation, without a runtime.
// Test assertions deliberately panic on invalid fixture construction.
#![allow(clippy::unwrap_used)]
use super::HiveMemory;
use crate::Error;
use openhuman_embed::{AgentSpec, RuntimeConfig};
use std::num::NonZeroU32;

#[test]
fn a_hive_root_is_its_team_namespace() {
    let memory = HiveMemory::for_hive("hive-7_a").unwrap();
    assert_eq!(memory.root(), "team:hive-7_a");
    assert_eq!(memory.budget(), None);
}

#[test]
fn an_explicit_nested_root_is_kept_trimmed() {
    let memory = HiveMemory::with_root("  project:q4/team:ops ").unwrap();
    assert_eq!(memory.root(), "project:q4/team:ops");
}

#[test]
fn a_seat_memory_agent_id_is_its_seat_id() {
    let memory = HiveMemory::for_hive("h").unwrap();
    assert_eq!(memory.agent_id("scout").unwrap(), "scout");
    let binding = memory.binding("scout").unwrap();
    assert_eq!(binding.agent_id(), "scout");
    assert_eq!(binding.root_namespace(), Some("team:h"));
}

#[test]
fn binding_a_spec_keeps_its_seat_id() {
    let memory = HiveMemory::for_hive("h").unwrap();
    let spec = memory.bind(AgentSpec::new("scout")).unwrap();
    assert_eq!(spec.id(), "scout");
    let debug = format!("{spec:?}");
    assert!(debug.contains("agent_id: \"scout\""), "{debug}");
    assert!(debug.contains("team:h"), "{debug}");
}

#[test]
fn rejects_a_hive_id_outside_the_segment_charset() {
    for hive in ["", "has space", "a/b", "team:x", &"x".repeat(129)] {
        let error = HiveMemory::for_hive(hive).unwrap_err();
        assert!(
            matches!(&error, Error::InvalidMemoryRoot { root, .. } if root == &format!("team:{hive}")),
            "{hive:?}: {error}"
        );
    }
    assert!(HiveMemory::for_hive(&"x".repeat(128)).is_ok());
}

#[test]
fn rejects_the_default_root_which_isolates_nothing() {
    for root in ["", "   ", "root"] {
        assert!(matches!(
            HiveMemory::with_root(root),
            Err(Error::InvalidMemoryRoot { .. })
        ));
    }
}

#[test]
fn rejects_a_root_openhuman_would_not_accept() {
    for root in ["not a namespace", "company:acme", "team:"] {
        let error = HiveMemory::with_root(root).unwrap_err();
        assert!(matches!(error, Error::InvalidMemoryRoot { .. }), "{root}");
        assert!(error.to_string().starts_with("invalid memory root"));
    }
}

#[test]
fn rejects_an_unusable_seat_id() {
    let memory = HiveMemory::for_hive("h").unwrap();
    for seat in ["", "Bad Seat!", "a:b"] {
        assert!(matches!(
            memory.agent_id(seat),
            Err(Error::InvalidMemoryAgentId { agent_id, .. }) if agent_id == seat
        ));
    }
    assert!(matches!(
        memory.bind(AgentSpec::new("bad seat")),
        Err(Error::InvalidMemoryAgentId { .. })
    ));
}

#[test]
fn configure_sets_the_recall_budget_only_when_given() {
    let mut config = RuntimeConfig::default();
    let default = config.memory.recall.budget_tokens;
    HiveMemory::for_hive("h").unwrap().configure(&mut config);
    assert_eq!(config.memory.recall.budget_tokens, default);
    let memory = HiveMemory::for_hive("h")
        .unwrap()
        .recall_budget_tokens(NonZeroU32::new(640).unwrap());
    assert_eq!(memory.budget(), NonZeroU32::new(640));
    memory.configure(&mut config);
    assert_eq!(config.memory.recall.budget_tokens, 640);
    assert_eq!(config.memory.agent_id, None);
    assert_eq!(config.memory.root, None);
}
