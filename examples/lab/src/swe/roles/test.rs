//! Prompt composition.

use super::*;

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
        let prompt = hive_system(role, "fix the parser");
        assert!(prompt.contains("fix the parser"));
        assert!(prompt.contains("no internet"));
    }
    assert!(single_system("t").contains("complete_episode"));
    assert!(hive_system(Role::Lead, "t").contains("!pin"));
}

#[test]
fn turn_message_names_the_seat() {
    assert!(hive_turn("tester", "## Recent", "Go.").contains("@tester"));
}
