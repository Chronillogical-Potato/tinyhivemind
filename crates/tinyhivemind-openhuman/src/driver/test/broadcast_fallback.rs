//! Per-author broadcast fallback regressions.

use super::*;

#[test]
fn all_broadcast_round_uses_a_distinct_valid_fallback_for_each_author() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 3).expect("driver");
    let state = driver
        .start(episode(&["one", "two", "three"]))
        .expect("state");
    let round = driver.pending_round(&state).expect("round");
    let route_policy = policy(3);
    let transition = fixture()
        .block_on(driver.apply_committed_round(
            &state,
            &round,
            vec![
                committed(
                    "one",
                    1,
                    Utterance::Broadcast {
                        message: "from one".into(),
                    },
                ),
                committed(
                    "two",
                    2,
                    Utterance::Broadcast {
                        message: "from two".into(),
                    },
                ),
                committed(
                    "three",
                    3,
                    Utterance::Broadcast {
                        message: "from three".into(),
                    },
                ),
            ],
            Some(BroadcastRouting {
                primary: None,
                reasoning: None,
                policy: &route_policy,
                roster_version: 1,
                thread_context: &[],
            }),
        ))
        .expect("all-broadcast round applies");

    let responders: Vec<_> = transition
        .actions
        .iter()
        .filter_map(|action| match action {
            HostAction::RunAgents {
                plan: RoutingPlan::Fallback { responder_id, .. },
                ..
            } => Some(responder_id.as_str()),
            HostAction::RunAgents { .. }
            | HostAction::DeliverDm { .. }
            | HostAction::DeliverHandoff { .. } => None,
        })
        .collect();
    let runs = transition
        .actions
        .iter()
        .filter(|action| matches!(action, HostAction::RunAgents { .. }))
        .count();
    assert_eq!(runs, 3, "one route per broadcast");
    // Each broadcast completed its author (ADR 0024). The handoffs the first
    // two broadcasts queued for the second and third authors -- each was still
    // working when named -- are handed over at those completions.
    assert_eq!(
        transition
            .actions
            .iter()
            .filter(|action| matches!(action, HostAction::DeliverHandoff { .. }))
            .count(),
        2
    );
    assert_eq!(responders, ["two", "three", "one"]);
    assert_eq!(transition.state.revision(), 3);
}

#[test]
fn broadcast_without_another_participant_is_rejected_before_router_call() {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, 1).expect("driver");
    let state = driver.start(episode(&["one"])).expect("state");
    let round = driver.pending_round(&state).expect("round");
    let calls = Arc::new(AtomicUsize::new(0));
    let router = RecordingRouter {
        calls: Arc::clone(&calls),
        widths: Arc::new(Mutex::new(Vec::new())),
    };
    let route_policy = policy(1);
    let result = fixture().block_on(driver.apply_committed_round(
        &state,
        &round,
        vec![committed(
            "one",
            1,
            Utterance::Broadcast {
                message: "anyone?".into(),
            },
        )],
        Some(BroadcastRouting {
            primary: Some(&router),
            reasoning: None,
            policy: &route_policy,
            roster_version: 1,
            thread_context: &[],
        }),
    ));

    assert!(matches!(
        result,
        Err(Error::NoBroadcastFallback { agent_id }) if agent_id == "one"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.revision(), 0);
}
