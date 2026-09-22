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
    assert_eq!(
        assigned.participants[0]
            .open()
            .map(|record| record.assigned_at),
        Some(Sequence(13))
    );
    assert_eq!(
        assigned.participants[1].assignments[0].completed_at,
        Some(Sequence(12))
    );
    // The reopened participant keeps the record it already finished, which is
    // what the append buys over the overwrite it replaces.
    assert_eq!(assigned.participants[0].assignments.len(), 2);
    assert_eq!(
        assigned.participants[0].assignments[0].completed_at,
        Some(Sequence(11))
    );
    assert_eq!(assigned.settled(), 2);
}

#[test]
fn duplicate_completion_is_idempotent_and_stale_events_are_rejected() {
    let once = apply_completion(&opened(), "solver", Sequence(11))
        .expect("solver completion advances the assignment");
    assert_eq!(
        apply_completion(&once, "solver", Sequence(11)).expect("exact replay is idempotent"),
        once
    );
    // The record is closed, so there is nothing to be stale against: the
    // single slot used to answer `Stale` here only because it had no way to
    // say the assignment was already settled.
    assert!(matches!(
        apply_completion(&once, "solver", Sequence(10)),
        Err(Error::NoOpenAssignment { .. })
    ));
    // A genuine stale completion is one that does not advance the assignment
    // it would close, and still reports as such.
    assert!(matches!(
        apply_completion(&opened(), "solver", Sequence(10)),
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
fn malformed_completion_and_assignment_events_have_typed_errors() {
    let state = opened();
    assert!(matches!(
        apply_completion(&state, "unknown", Sequence(11)),
        Err(Error::UnknownCompletionParticipant { .. })
    ));
    assert!(matches!(
        apply_assignment(&state, [] as [&str; 0], Sequence(11)),
        Err(Error::InvalidCompletionParticipant)
    ));
    assert!(matches!(
        apply_assignment(&state, [" "], Sequence(11)),
        Err(Error::InvalidCompletionParticipant)
    ));
    assert!(matches!(
        apply_assignment(&state, ["solver", "solver"], Sequence(11)),
        Err(Error::DuplicateCompletionParticipant { .. })
    ));
    // An open recipient is refused before the sequence is even considered.
    assert!(matches!(
        apply_assignment(&state, ["solver"], Sequence(10)),
        Err(Error::AssignmentWhileOpen { .. })
    ));
    // Settled, and the assignment still has to advance past the last one.
    let settled = apply_completion(&state, "solver", Sequence(11)).expect("solver is assigned");
    assert!(matches!(
        apply_assignment(&settled, ["solver"], Sequence(10)),
        Err(Error::StaleCompletionEvent { .. })
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
                {"agent_id":"solver", "assignments":[{"assigned_at":10,"completed_at":null}]},
                {"agent_id":"checker", "assignments":[{"assigned_at":10,"completed_at":null}]}
            ]
        })
    );
}

#[test]
fn an_assignment_is_refused_while_the_recipient_still_has_work_open() {
    let open = opened();
    let refused = apply_assignment(&open, ["solver"], Sequence(13));
    assert!(matches!(
        refused,
        Err(Error::AssignmentWhileOpen {
            ref agent_id,
            assigned_at: Sequence(10),
        }) if agent_id == "solver"
    ));
    // Nothing moved: the refusal is a precondition, not a partial write.
    assert_eq!(status(&open), status(&opened()));
    // The same recipient is assignable the moment it has nothing open.
    let done = apply_completion(&open, "solver", Sequence(11)).expect("solver is assigned");
    apply_assignment(&done, ["solver"], Sequence(13))
        .expect("a settled participant may be reopened");
}

#[test]
fn a_refused_recipient_does_not_leave_its_peers_assigned() {
    let done = apply_completion(&opened(), "solver", Sequence(11)).expect("solver is assigned");
    // `checker` is still working, so the whole round is refused and `solver`
    // does not quietly acquire the assignment its peer could not take.
    assert!(matches!(
        apply_assignment(&done, ["solver", "checker"], Sequence(13)),
        Err(Error::AssignmentWhileOpen { .. })
    ));
    assert_eq!(done.participants[0].assignments.len(), 1);
}

#[test]
fn settled_never_decreases_across_a_reopen() {
    let mut state = opened();
    assert_eq!(state.settled(), 0);
    state = apply_completion(&state, "solver", Sequence(11)).expect("solver is assigned");
    assert_eq!(state.settled(), 1);
    state = apply_completion(&state, "checker", Sequence(12)).expect("checker is assigned");
    assert_eq!(state.settled(), 2);
    // A reopen is the one operation that used to erase a completion.
    state = apply_assignment(&state, ["solver"], Sequence(13)).expect("solver is settled");
    assert_eq!(state.settled(), 2);
    state = apply_completion(&state, "solver", Sequence(14)).expect("solver is assigned again");
    assert_eq!(state.settled(), 3);
}

#[test]
fn an_assignment_below_a_recorded_completion_cannot_erase_it() {
    let done = apply_completion(&opened(), "solver", Sequence(19)).expect("solver is assigned");
    // Under the single slot this passed -- the check read `assigned_at` (10)
    // and never `completed_at` (19) -- and cleared a completion recorded later
    // than the assignment replacing it. The record is now its own subject.
    let reassigned =
        apply_assignment(&done, ["solver"], Sequence(15)).expect("15 is past the assignment at 10");
    assert_eq!(
        reassigned.participants[0].assignments[0].completed_at,
        Some(Sequence(19))
    );
    assert_eq!(reassigned.settled(), 1);
}

#[test]
fn a_payload_from_before_the_history_fails_to_decode() {
    // The wire convention `refutation_cap` set: an older payload is an error
    // rather than a participant that silently acquires one empty history.
    let legacy = serde_json::json!({
        "conversation": {"desk_id":"math", "desk_name":"Mathematics", "thread_root":null},
        "watermark": 10,
        "participants": [{"agent_id":"solver", "assigned_at":10, "completed_at":null}]
    });
    assert!(serde_json::from_value::<CompletionEpisodeState>(legacy).is_err());
}
