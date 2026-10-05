//! Parked release, durable reattachment, cancellation, and live shutdown.
#![allow(clippy::unwrap_used)]
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn parked_episode_and_direct_turn_resume_only_after_release() {
    for destination in [
        Destination::Hive("work".into()),
        Destination::Agent("a".into()),
    ] {
        let c = setup().await;
        let action = c.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        add(&c, "a", move |request| {
            let c = action.clone();
            let count = count.clone();
            Box::pin(async move {
                let index = count.fetch_add(1, Ordering::SeqCst);
                let mut outcome = done(&request);
                if index == 0 {
                    outcome.disposition = TurnDisposition::Parked;
                } else if let Some(ep) = &request.episode {
                    c.submit_action(
                        "a",
                        &ep.episode_id,
                        EpisodeAction::Complete {
                            body: "approved".into(),
                        },
                    )
                    .await?;
                }
                Ok(outcome)
            })
        })
        .await;
        hive(&c, "work", &["a"]).await;
        c.send_as_host(message("task", destination)).await.unwrap();
        assert_eq!(c.run_until_idle().await.unwrap().parked, 1);
        assert_eq!(c.run_until_idle().await.unwrap().completed, 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        c.release("a").await.unwrap();
        assert_eq!(c.run_until_idle().await.unwrap().completed, 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
#[tokio::test]
async fn recovery_accepts_new_runtime_and_retains_session_and_pending_messages() {
    let storage = Arc::new(MemoryStorage::new());
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    c.send_as_host(message("first", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    c.send_as_host(message("second", Destination::Agent("a".into())))
        .await
        .unwrap();
    let recovered = Coordinator::new("new-runtime".into(), storage, CoordinatorOptions::default())
        .await
        .unwrap();
    assert_eq!(recovered.run_until_idle().await.unwrap().completed, 0);
    recovered
        .register_agent(AgentRegistration {
            agent_id: "a".into(),
            runtime_id: "new-runtime".into(),
            runner: Arc::new(Script(Arc::new(|request| {
                Box::pin(async move {
                    assert_eq!(request.session_id.as_deref(), Some("session:a"));
                    assert_eq!(request.messages[0].message_id, "second");
                    Ok(done(&request))
                })
            }))),
        })
        .await
        .unwrap();
    assert_eq!(recovered.run_until_idle().await.unwrap().completed, 1);
}
#[tokio::test]
async fn dropped_drain_records_interruption_and_never_replays_started_turn() {
    let storage = Arc::new(MemoryStorage::new());
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    add(&c, "a", move |_| {
        let signal = signal.clone();
        Box::pin(async move {
            signal.notify_one();
            std::future::pending().await
        })
    })
    .await;
    c.send_as_host(message("uncertain", Destination::Agent("a".into())))
        .await
        .unwrap();
    let mut drain = Box::pin(c.run_until_idle());
    tokio::select! { () = started.notified() => {}, result = &mut drain => { assert!(result.is_err()); } }
    drop(drain);
    let interruptions = c.interruptions().unwrap();
    assert_eq!(interruptions.len(), 1);
    assert_eq!(interruptions[0].message_ids, ["uncertain"]);
    assert_eq!(c.run_until_idle().await.unwrap().completed, 0);
    let recovered = Coordinator::new("new".into(), storage, CoordinatorOptions::default())
        .await
        .unwrap();
    assert_eq!(recovered.interruptions().unwrap().len(), 1);
    assert_eq!(recovered.run_until_idle().await.unwrap().completed, 0);
}
#[tokio::test]
async fn durable_running_claim_is_interrupted_on_crash_recovery() {
    let storage = Arc::new(MemoryStorage::new());
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    c.send_as_host(message("uncertain", Destination::Agent("a".into())))
        .await
        .unwrap();
    let claims = c.claim(1).await.unwrap();
    assert_eq!(claims.len(), 1);
    let recovered = Coordinator::new("new".into(), storage, CoordinatorOptions::default())
        .await
        .unwrap();
    assert_eq!(recovered.interruptions().unwrap().len(), 1);
    assert_eq!(recovered.run_until_idle().await.unwrap().completed, 0);
}
#[tokio::test]
async fn run_wakes_on_dynamic_registration_and_shutdown_waits_for_active_turn() {
    let c = setup().await;
    let started = Arc::new(tokio::sync::Notify::new());
    let finish = Arc::new(tokio::sync::Notify::new());
    let mut running = Box::pin(c.run());
    assert!(matches!(
        futures::poll!(&mut running),
        std::task::Poll::Pending
    ));
    let signal = started.clone();
    let wait = finish.clone();
    add(&c, "a", move |request| {
        let signal = signal.clone();
        let wait = wait.clone();
        Box::pin(async move {
            signal.notify_one();
            wait.notified().await;
            Ok(done(&request))
        })
    })
    .await;
    hive(&c, "dynamic", &["a"]).await;
    c.send_as_host(message("one", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.send_as_host(message("two", Destination::Agent("a".into())))
        .await
        .unwrap();
    tokio::select! { () = started.notified() => {}, result = &mut running => { assert!(result.is_err()); } }
    c.shutdown();
    assert!(matches!(
        futures::poll!(&mut running),
        std::task::Poll::Pending
    ));
    finish.notify_one();
    running.await.unwrap();
    assert_eq!(
        c.lock().unwrap().durable.deliveries[0].status,
        crate::DeliveryStatus::Delivered
    );
    assert_eq!(
        c.lock().unwrap().durable.deliveries[1].status,
        crate::DeliveryStatus::Pending
    );
}
#[tokio::test]
async fn supplied_session_binding_is_idempotent_and_cannot_switch_history() {
    let c = setup().await;
    add(&c, "a", |request| {
        Box::pin(async move {
            assert_eq!(request.session_id.as_deref(), Some("existing-session"));
            Ok(done(&request))
        })
    })
    .await;
    c.bind_session("a", "existing-session").await.unwrap();
    c.bind_session("a", "existing-session").await.unwrap();
    assert!(matches!(
        c.bind_session("a", "another").await,
        Err(Error::SessionConflict(_))
    ));
    assert!(c.bind_session("absent", "session").await.is_err());
    assert!(c.bind_session("a", "").await.is_err());
    c.send_as_host(message("task", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    c.bind_session("a", "existing-session").await.unwrap();
}
#[tokio::test]
async fn runner_cannot_replace_a_bound_continuing_session() {
    let c = setup().await;
    add(&c, "a", |_| {
        Box::pin(async {
            Ok(TurnOutcome {
                session_id: "other".into(),
                reply: None,
                disposition: TurnDisposition::Completed,
            })
        })
    })
    .await;
    c.bind_session("a", "existing").await.unwrap();
    c.send_as_host(message("task", Destination::Agent("a".into())))
        .await
        .unwrap();
    let report = c.run_until_idle().await.unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(
        c.lock().unwrap().durable.agents["a"].session_id.as_deref(),
        Some("existing")
    );
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_reopens_conductor_checkpoint_and_resumes_parked_agent() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hives.sqlite");
    let storage = Arc::new(crate::SqliteStorage::open(&path).unwrap());
    let c = Coordinator::new("runtime".into(), storage, CoordinatorOptions::default())
        .await
        .unwrap();
    let c2 = c.clone();
    add(&c, "a", move |request| {
        let c = c2.clone();
        Box::pin(async move {
            c.submit_action(
                "a",
                &request.episode.as_ref().unwrap().episode_id,
                EpisodeAction::Post {
                    body: "waiting".into(),
                },
            )
            .await?;
            let mut outcome = done(&request);
            outcome.disposition = TurnDisposition::Parked;
            Ok(outcome)
        })
    })
    .await;
    hive(&c, "work", &["a"]).await;
    c.send_as_host(message("task", Destination::Hive("work".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    let recovered = Coordinator::new(
        "restarted".into(),
        Arc::new(crate::SqliteStorage::open(&path).unwrap()),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    let c2 = recovered.clone();
    recovered
        .register_agent(AgentRegistration {
            agent_id: "a".into(),
            runtime_id: "restarted".into(),
            runner: Arc::new(Script(Arc::new(move |request| {
                let c = c2.clone();
                Box::pin(async move {
                    assert_eq!(request.session_id.as_deref(), Some("session:a"));
                    c.submit_action(
                        "a",
                        &request.episode.as_ref().unwrap().episode_id,
                        EpisodeAction::Complete {
                            body: "approved".into(),
                        },
                    )
                    .await?;
                    Ok(done(&request))
                })
            }))),
        })
        .await
        .unwrap();
    recovered.release("a").await.unwrap();
    assert_eq!(recovered.run_until_idle().await.unwrap().completed, 1);
    assert!(recovered.lock().unwrap().durable.episodes[0].finished);
    assert!(
        recovered
            .read_hive("a", "work", None, None)
            .unwrap()
            .iter()
            .any(|m| m.body == "waiting")
    );
}
