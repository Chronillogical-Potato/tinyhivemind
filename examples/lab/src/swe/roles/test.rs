//! Prompt composition.

use super::*;
use crate::swe::session::SessionMode;

#[test]
fn lead_is_first_and_ids_round_trip() {
    assert_eq!(Role::ALL[0], Role::Lead);
    for role in Role::ALL {
        assert_eq!(Role::from_id(role.id()), Some(role));
    }
    assert_eq!(Role::from_id("nobody"), None);
}

#[test]
fn every_prompt_carries_the_task_and_environment() {
    for role in Role::ALL {
        let prompt = hive_system(role, "fix the parser", SessionMode::Persistent);
        assert!(prompt.contains("fix the parser"));
        assert!(prompt.contains("no internet"));
    }
    assert!(single_system("t").contains("complete_episode"));
    assert!(hive_system(Role::Lead, "t", SessionMode::Fresh).contains("!pin"));
}

#[test]
fn turn_message_names_the_seat() {
    assert!(hive_turn("tester", "## Recent", "Go.").contains("@tester"));
}

#[test]
fn the_rules_tell_a_persistent_seat_it_continues_its_session() {
    let persistent = hive_system(Role::Tester, "t", SessionMode::Persistent);
    assert!(persistent.contains("continue your own conversation"));
    let fresh = hive_system(Role::Tester, "t", SessionMode::Fresh);
    assert!(fresh.contains("starts from the desk, not from this conversation"));
}

#[test]
fn a_rejoin_message_says_the_session_continues() {
    let text = hive_rejoin("tester", "## New on the desk", "@lead addressed you.");
    assert!(text.contains("@tester") && text.contains("continuing your session"));
    assert!(text.starts_with("## New on the desk"));
}
