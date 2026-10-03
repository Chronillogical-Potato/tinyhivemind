//! Global exclusion, bounded concurrency, FIFO ordering, and active snapshots.
#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn different_agents_run_concurrently_but_shared_agents_are_globally_serialized() {
    let c = Coordinator::new(
        "runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions {
            round_width: 2,
            ..CoordinatorOptions::default()
        },
    )
    .unwrap();
    let started = Arc::new(tokio::sync::Barrier::new(3));
    let finish = Arc::new(tokio::sync::Semaphore::new(0));
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    for id in ["a", "b"] {
        let started = started.clone();
        let finish = finish.clone();
        let seen = seen.clone();
        add(&c, id, move |request| {
            let started = started.clone();
            let finish = finish.clone();
            let seen = seen.clone();
            Box::pin(async move {
                seen.lock().unwrap().push((
                    request.agent_id.clone(),
                    request.messages[0].message_id.clone(),
                    request.session_id.clone(),
                ));
                if request.session_id.is_none() {
                    started.wait().await;
                    let _permit = finish.acquire().await.unwrap();
                }
                Ok(done(&request))
            })
        });
    }
    for (id, agent) in [("first-a", "a"), ("second-a", "a"), ("first-b", "b")] {
        c.send_as_host(message(id, Destination::Agent(agent.into())))
            .unwrap();
    }
    let mut drain = Box::pin(c.run_until_idle());
    tokio::select! { _ = started.wait() => {}, result = &mut drain => { assert!(result.is_err()); } }
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert_eq!(c.lock().unwrap().durable.running.len(), 2);
    finish.add_permits(2);
    assert_eq!(drain.await.unwrap().completed, 3);
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].0, "a");
    assert_eq!(seen[1].0, "b");
    assert_eq!(seen[2].1, "second-a");
    assert_eq!(seen[2].2.as_deref(), Some("session:a"));
}
#[tokio::test]
async fn active_turn_keeps_membership_snapshot_after_leave_and_new_turn_is_blocked() {
    let c = setup();
    let started = Arc::new(tokio::sync::Notify::new());
    let finish = Arc::new(tokio::sync::Notify::new());
    let c2 = c.clone();
    let signal = started.clone();
    let wait = finish.clone();
    add(&c, "a", move |request| {
        let c = c2.clone();
        let signal = signal.clone();
        let wait = wait.clone();
        Box::pin(async move {
            signal.notify_one();
            wait.notified().await;
            assert_eq!(request.memberships[0].members, ["a"]);
            assert_eq!(c.read_hive("a", "work", None, None)?.len(), 1);
            c.submit_action(
                "a",
                &request.episode.as_ref().unwrap().episode_id,
                EpisodeAction::Complete {
                    body: "done".into(),
                },
            )?;
            Ok(done(&request))
        })
    });
    hive(&c, "work", &["a"]);
    c.send_as_host(message("task", Destination::Hive("work".into())))
        .unwrap();
    let mut drain = Box::pin(c.run_until_idle());
    tokio::select! { () = started.notified() => {}, result = &mut drain => { assert!(result.is_err()); } }
    c.leave_hive("work", "a").unwrap();
    finish.notify_one();
    assert_eq!(drain.await.unwrap().completed, 1);
    assert!(c.read_hive("a", "work", None, None).is_err());
}
#[tokio::test]
async fn conductor_bounds_rounds_and_stops_silent_agents_at_existing_walls() {
    let c = Coordinator::new(
        "runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions {
            conduct_policy: tinyhivemind_core::driver::ConductPolicy {
                turn_wall: 2,
                child_turn_wall: 1,
            },
            ..CoordinatorOptions::default()
        },
    )
    .unwrap();
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    });
    hive(&c, "work", &["a"]);
    c.send_as_host(message("silent", Destination::Hive("work".into())))
        .unwrap();
    let report = c.run_until_idle().await.unwrap();
    assert_eq!(report.completed, 2);
    assert_eq!(report.failed, 1);
    let state = c.lock().unwrap();
    assert!(state.durable.episodes[0].finished);
    assert!(
        state.durable.episodes[0]
            .failure
            .as_ref()
            .unwrap()
            .contains("wall")
    );
}
#[tokio::test]
async fn zero_broadcast_budget_discharges_the_assignment() {
    let c = Coordinator::new(
        "runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions {
            broadcast_budget: Some(0),
            ..CoordinatorOptions::default()
        },
    )
    .unwrap();
    let c2 = c.clone();
    add(&c, "a", move |request| {
        let c = c2.clone();
        Box::pin(async move {
            c.submit_action(
                "a",
                &request.episode.as_ref().unwrap().episode_id,
                EpisodeAction::Broadcast {
                    body: "delegate".into(),
                },
            )?;
            Ok(done(&request))
        })
    });
    hive(&c, "work", &["a"]);
    c.send_as_host(message("task", Destination::Hive("work".into())))
        .unwrap();
    assert_eq!(c.run_until_idle().await.unwrap().completed, 1);
    assert!(
        c.read_hive("a", "work", None, None)
            .unwrap()
            .iter()
            .any(|m| m.body.contains("budget spent"))
    );
}
#[tokio::test]
async fn replies_are_attributed_and_hive_threads_filter_visible_rows() {
    let c = setup();
    add(&c, "a", |request| {
        Box::pin(async move {
            let mut outcome = done(&request);
            outcome.reply = Some("reply".into());
            Ok(outcome)
        })
    });
    c.send_as_host(message("direct", Destination::Agent("a".into())))
        .unwrap();
    c.run_until_idle().await.unwrap();
    assert_eq!(c.lock().unwrap().durable.messages[1].sender, "a");
    assert_eq!(c.lock().unwrap().durable.messages[1].body, "reply");
    hive(&c, "work", &["a"]);
    let root = c
        .send(message("root", Destination::Hive("work".into())))
        .unwrap();
    let mut follow = message("follow", Destination::Hive("work".into()));
    follow.thread = Some(root.sequence);
    c.send(follow).unwrap();
    let rows = c.read_hive("a", "work", None, Some(root.sequence)).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].thread, Some(root.sequence));
}
#[test]
fn destinations_require_membership_and_reserved_message_ids_cannot_collide() {
    let c = setup();
    for id in ["a", "b"] {
        add(&c, id, |request| {
            Box::pin(async move { Ok(done(&request)) })
        });
    }
    hive(&c, "work", &["b"]);
    assert!(
        c.send(message("forbidden", Destination::Hive("work".into())))
            .is_err()
    );
    let mut private = message("private", Destination::Hive("work".into()));
    private.sender = "b".into();
    private.only_for = vec!["a".into()];
    assert!(c.send(private).is_err());
    let mut direct = message("threaded", Destination::Agent("b".into()));
    direct.thread = Some(0);
    assert!(c.send(direct).is_err());
    assert!(
        c.send_as_host(message("hivemind:event:0", Destination::Agent("b".into())))
            .is_err()
    );
    c.join_hive("work", "a").unwrap();
    c.join_hive("work", "a").unwrap();
    assert_eq!(c.list_agents().unwrap(), ["a", "b"]);
}
