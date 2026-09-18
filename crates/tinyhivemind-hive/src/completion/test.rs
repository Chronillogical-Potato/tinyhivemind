//! Completion-driven episode behavior and wire forms.

#![allow(clippy::expect_used)]

use tinyhivemind::{Conversation, Sequence};

use super::*;

fn opened() -> CompletionEpisodeState {
    CompletionEpisodeState::opened(
        Conversation {
            desk_id: "math".into(),
            desk_name: "Mathematics".into(),
            thread_root: None,
        },
        Sequence(10),
        ["solver", "checker"],
    )
    .expect("unique nonblank participants")
}

#[test]
fn an_episode_completes_only_after_every_assigned_agent_calls_completion() {
    let solver_done = apply_completion(&opened(), "solver", Sequence(11))
        .expect("solver is assigned to the episode");
    assert_eq!(
        status(&solver_done),
        CompletionStep::Active {
            pending_ids: vec!["checker".into()]
        }
    );
    let all_done = apply_completion(&solver_done, "checker", Sequence(12))
        .expect("checker is assigned to the episode");
    assert_eq!(
        status(&all_done),
        CompletionStep::Complete {
            completed_ids: vec!["solver".into(), "checker".into()]
        }
    );
}

#[test]
fn a_routed_broadcast_reopens_only_the_agents_who_received_work() {
    let solver_done = apply_completion(&opened(), "solver", Sequence(11))
        .expect("solver completion advances the assignment");
    let all_done = apply_completion(&solver_done, "checker", Sequence(12))
        .expect("checker completion advances the assignment");
    let assigned = apply_assignment(&all_done, ["solver"], Sequence(13))
        .expect("an existing participant may receive another assignment");
    assert_eq!(
        status(&assigned),
        CompletionStep::Active {
            pending_ids: vec!["solver".into()]
        }
    );
    assert_eq!(assigned.participants[0].assigned_at, Sequence(13));
    assert_eq!(assigned.participants[1].completed_at, Some(Sequence(12)));
}

#[test]
fn duplicate_completion_is_idempotent_and_stale_events_are_rejected() {
    let once = apply_completion(&opened(), "solver", Sequence(11))
        .expect("solver completion advances the assignment");
    assert_eq!(
        apply_completion(&once, "solver", Sequence(11)).expect("exact replay is idempotent"),
        once
    );
    assert!(matches!(
        apply_completion(&once, "solver", Sequence(10)),
        Err(Error::StaleCompletionEvent { .. })
    ));
    assert!(matches!(
        apply_assignment(&once, ["unknown"], Sequence(12)),
        Err(Error::UnknownCompletionParticipant { .. })
    ));
}

#[test]
fn malformed_participant_sets_have_typed_errors() {
    let conversation = opened().conversation;
    assert!(matches!(
        CompletionEpisodeState::opened(conversation.clone(), Sequence(1), [] as [&str; 0]),
        Err(Error::NoCompletionParticipants)
    ));
    assert!(matches!(
        CompletionEpisodeState::opened(conversation.clone(), Sequence(1), [" "]),
        Err(Error::InvalidCompletionParticipant)
    ));
    assert!(matches!(
        CompletionEpisodeState::opened(conversation, Sequence(1), ["solver", "solver"]),
        Err(Error::DuplicateCompletionParticipant { .. })
    ));
}

#[test]
fn state_has_a_stable_wire_shape() {
    let state = opened();
    assert_eq!(
        serde_json::to_value(state).expect("state serializes"),
        serde_json::json!({
            "conversation": {"desk_id":"math", "desk_name":"Mathematics", "thread_root":null},
            "watermark": 10,
            "participants": [
                {"agent_id":"solver", "assigned_at":10, "completed_at":null},
                {"agent_id":"checker", "assigned_at":10, "completed_at":null}
            ]
        })
    );
}
