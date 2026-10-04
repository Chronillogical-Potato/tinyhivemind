//! Boundary errors and failed-turn isolation.
#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn failed_runner_does_not_strand_other_agents_or_replay_failed_work() {
    let c = setup().await;
    add(&c, "a", |_| {
        Box::pin(async { Err(Error::InvalidIdentifier("scripted failure")) })
    })
    .await;
    add(&c, "b", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    c.send_as_host(message("bad", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.send_as_host(message("good", Destination::Agent("b".into())))
        .await
        .unwrap();
    let report = c.run_until_idle().await.unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(report.completed, 1);
    assert_eq!(c.interruptions().unwrap().len(), 1);
    assert_eq!(c.lock().unwrap().durable.running.len(), 0);
    assert_eq!(c.run_until_idle().await.unwrap().completed, 0);
}
#[tokio::test]
async fn rejects_invalid_options_definitions_destinations_and_stale_actions() {
    assert!(
        Coordinator::new(
            String::new(),
            Arc::new(MemoryStorage::new()),
            CoordinatorOptions::default()
        )
        .await
        .is_err()
    );
    assert!(
        Coordinator::new(
            "runtime".into(),
            Arc::new(MemoryStorage::new()),
            CoordinatorOptions {
                round_width: 0,
                ..CoordinatorOptions::default()
            }
        )
        .await
        .is_err()
    );
    let c = setup().await;
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    assert!(
        c.create_hive(HiveInfo {
            hive_id: "unknown".into(),
            name: "Unknown".into(),
            description: None,
            members: vec!["absent".into()]
        })
        .await
        .is_err()
    );
    assert!(
        c.create_hive(HiveInfo {
            hive_id: "duplicate".into(),
            name: "Duplicate".into(),
            description: None,
            members: vec!["a".into(), "a".into()]
        })
        .await
        .is_err()
    );
    hive(&c, "work", &["a"]).await;
    assert!(
        c.create_hive(HiveInfo {
            hive_id: "work".into(),
            name: "Changed".into(),
            description: None,
            members: vec!["a".into()]
        })
        .await
        .is_err()
    );
    assert!(c.join_hive("missing", "a").await.is_err());
    assert!(c.leave_hive("work", "missing").await.is_err());
    assert!(
        c.send_as_host(message("missing", Destination::Agent("missing".into())))
            .await
            .is_err()
    );
    assert!(
        c.send_as_host(message("missing-hive", Destination::Hive("missing".into())))
            .await
            .is_err()
    );
    assert!(
        c.submit_action(
            "a",
            "stale",
            EpisodeAction::Complete {
                body: "done".into()
            }
        )
        .await
        .is_err()
    );
    assert!(c.release("missing").await.is_err());
    let mut forged = message("forged", Destination::Agent("a".into()));
    forged.sender = HOST_ID.into();
    assert!(c.send(forged).await.is_err());
}
#[tokio::test]
async fn diagnostics_and_wire_payloads_preserve_public_identities() {
    let c = setup().await;
    assert_eq!(c.runtime_id(), "runtime");
    assert!(format!("{c:?}").contains("runtime"));
    let registration = AgentRegistration {
        agent_id: "a".into(),
        runtime_id: "runtime".into(),
        runner: Arc::new(Script(Arc::new(|request| {
            Box::pin(async move { Ok(done(&request)) })
        }))),
    };
    assert!(format!("{registration:?}").contains("agent_id"));
    assert_eq!(
        serde_json::to_value(message("m", Destination::Hive("work".into()))).unwrap(),
        serde_json::json!({"message_id":"m","sender":"a","destination":{"Hive":"work"},"body":"m","thread":null,"only_for":[]})
    );
}
#[tokio::test]
async fn failed_disposition_and_empty_session_release_reservations() {
    for disposition in [
        TurnDisposition::Failed("failed".into()),
        TurnDisposition::Completed,
    ] {
        let c = setup().await;
        let disposition = disposition.clone();
        add(&c, "a", move |_| {
            let disposition = disposition.clone();
            Box::pin(async move {
                Ok(TurnOutcome {
                    session_id: String::new(),
                    reply: None,
                    disposition,
                })
            })
        })
        .await;
        c.send_as_host(message("bad", Destination::Agent("a".into())))
            .await
            .unwrap();
        c.run_until_idle().await.unwrap();
        assert_eq!(c.interruptions().unwrap().len(), 1);
        assert_eq!(c.lock().unwrap().durable.running.len(), 0);
    }
}
#[tokio::test]
async fn active_episode_admits_only_bound_members_and_current_assignment() {
    let c = setup().await;
    let c2 = c.clone();
    add(&c, "a", move |request| {
        let c = c2.clone();
        Box::pin(async move {
            let ep = &request.episode.as_ref().unwrap().episode_id;
            assert!(
                c.submit_action(
                    "a",
                    "other-episode",
                    EpisodeAction::Complete {
                        body: "forged".into()
                    }
                )
                .await
                .is_err()
            );
            for agents in [vec![], vec!["unknown".into()], vec!["a".into()]] {
                assert!(
                    c.submit_action(
                        "a",
                        ep,
                        EpisodeAction::Ask {
                            agents,
                            body: "question".into()
                        }
                    )
                    .await
                    .is_err()
                );
            }
            assert!(c.bind_session("a", "switched").await.is_err());
            c.submit_action(
                "a",
                ep,
                EpisodeAction::Post {
                    body: "progress".into(),
                },
            )
            .await?;
            c.submit_action(
                "a",
                ep,
                EpisodeAction::Complete {
                    body: "done".into(),
                },
            )
            .await?;
            let mut result = done(&request);
            result.reply = Some("text reply".into());
            Ok(result)
        })
    })
    .await;
    hive(&c, "work", &["a"]).await;
    c.send_as_host(message("task", Destination::Hive("work".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    let rows = c.read_hive("a", "work", Some(0), None).unwrap();
    assert!(rows.iter().any(|m| m.body == "progress"));
    assert!(rows.iter().any(|m| m.body == "text reply"));
    assert!(c.read_hive("a", "work", None, Some(999)).is_err());
}
#[tokio::test]
async fn sequence_exhaustion_rejects_acceptance_atomically() {
    let storage = Arc::new(MemoryStorage::new());
    let state = crate::StoredState {
        revision: 1,
        next_sequence: u64::MAX,
        ..crate::StoredState::default()
    };
    storage
        .commit(crate::Commit {
            expected_revision: 0,
            state: &state,
            appended: &[],
        })
        .await
        .unwrap();
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
    assert!(matches!(
        c.send_as_host(message("full", Destination::Agent("a".into())))
            .await,
        Err(Error::Exhausted)
    ));
    assert_eq!(storage.load().await.unwrap().messages.len(), 0);
}
#[tokio::test]
async fn joining_after_episode_creation_cannot_inject_an_unbound_child_participant() {
    let c = setup().await;
    let c2 = c.clone();
    add(&c, "a", move |request| {
        let c = c2.clone();
        Box::pin(async move {
            let ep = &request.episode.as_ref().unwrap().episode_id;
            assert!(
                c.submit_action(
                    "a",
                    ep,
                    EpisodeAction::Ask {
                        agents: vec!["b".into()],
                        body: "late join".into()
                    }
                )
                .await
                .is_err()
            );
            c.submit_action(
                "a",
                ep,
                EpisodeAction::Complete {
                    body: "done".into(),
                },
            )
            .await?;
            Ok(done(&request))
        })
    })
    .await;
    add(&c, "b", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    hive(&c, "work", &["a"]).await;
    c.send_as_host(message("task", Destination::Hive("work".into())))
        .await
        .unwrap();
    c.join_hive("work", "b").await.unwrap();
    c.run_until_idle().await.unwrap();
}
