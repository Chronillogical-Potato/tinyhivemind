//! The door: who starts, and every shape a plan takes.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::support::{ClarifyRouter, Journal, complete, door, hive, policy, run, seats, wave};
use crate::CompletionDriver;
use crate::conduct::{ConductPolicy, Conductor, starters};
use crate::driver::BroadcastRouting;
use tinyhivemind_embed::{Router, RoutingFallback, RoutingPlan, RoutingRequest};

#[test]
fn the_door_starts_the_routed_seats_and_completes_the_rest() {
    let hive = hive(&["one", "two", "three"]);
    let driver = CompletionDriver::new(&hive, 4).expect("driver");
    let route_policy = policy(1);
    let routing = BroadcastRouting {
        primary: None,
        reasoning: None,
        policy: &route_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let journal = Journal::default();
    let mut conductor = Conductor::open(
        &driver,
        routing,
        ConductPolicy::default(),
        door(&["one", "two", "three"], &["one"], &journal),
    )
    .expect("opens");
    assert!(!conductor.finished());
    assert_eq!(conductor.chat(), "engineering");
    assert!(format!("{conductor:?}").contains("engineering"));

    let first = wave(&mut conductor, &journal, &[("one", vec![complete("done")])]).expect("wave");
    assert_eq!(
        seats(&first.turns),
        vec![("one", None)],
        "only the starter runs"
    );
    assert!(conductor.finished(), "one seat's completion ends it");
    assert_eq!(conductor.turns_run(), 1);
    assert_eq!(conductor.waves(), 1);
    assert_eq!(conductor.discharged(), 0);
    assert_eq!(conductor.conversations(), 0);
    assert_eq!(journal.bodies(), vec!["the task", "COMPLETE: done"]);
    assert!(conductor.state().quiescent());
}

#[test]
fn starters_are_read_from_every_plan_shape() {
    let evaluation = run(ClarifyRouter.evaluate(&RoutingRequest {
        message: String::new(),
        source: tinyhivemind_embed::RoutingSource::DeskMessage,
        conversation: tinyhivemind_embed::ConversationRef {
            id: "engineering".into(),
            kind: tinyhivemind_embed::ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: None,
        thread_context: Vec::new(),
        candidates: hive(&["a", "b"]).graph().candidates.clone(),
        roster_version: 1,
        policy: policy(1),
    }))
    .expect("evaluates");
    let one = RoutingPlan::One {
        responder_id: "a".into(),
        evaluation: evaluation.clone(),
    };
    let fallback = RoutingPlan::Fallback {
        responder_id: "b".into(),
        reason: RoutingFallback::ProviderUnavailable,
    };
    let hive = RoutingPlan::Hive {
        primary_id: "c".into(),
        invited_ids: vec!["d".into(), "e".into()],
        evaluation: evaluation.clone(),
    };
    let clarify = RoutingPlan::Clarify { evaluation };
    assert_eq!(starters(&one, "z"), vec!["a"]);
    assert_eq!(starters(&fallback, "z"), vec!["b"]);
    assert_eq!(starters(&hive, "z"), vec!["c", "d", "e"]);
    assert_eq!(starters(&clarify, "z"), vec!["z"]);
}
