//! Reservation admission and initiating audience regressions.
use super::*;

#[tokio::test]
async fn leaving_between_prepare_and_claim_retires_the_pending_turn() {
    let c = setup().await;
    add(&c, "a", |_| {
        Box::pin(async { panic!("removed member ran") })
    }).await;
    hive(&c, "work", &["a"]).await;
    c.send_as_host(message("task", Destination::Hive("work".into()))).await
        .unwrap();
    assert!(c.advance().await.unwrap());
    assert_eq!(c.lock().unwrap().durable.episodes[0].pending.len(), 1);
    c.leave_hive("work", "a").await.unwrap();
    assert!(c.claim(1).await.unwrap().is_empty());
    assert!(c.lock().unwrap().durable.running.is_empty());
    assert_eq!(c.lock().unwrap().durable.episodes[0].pending.len(), 0);
    c.run_until_idle().await.unwrap();
    assert!(c.lock().unwrap().durable.episodes[0].finished);
}

async fn private_initial_contract(storage: Arc<dyn crate::Storage>) {
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    ).await
    .unwrap();
    for id in ["a", "b", "outsider"] {
        let coordinator = c.clone();
        add(&c, id, move |request| {
            let c = coordinator.clone();
            Box::pin(async move {
                c.submit_action(
                    &request.agent_id,
                    &request.episode.as_ref().unwrap().episode_id,
                    EpisodeAction::Post {
                        body: "secret post".into(),
                    },
                )?;
                c.submit_action(
                    &request.agent_id,
                    &request.episode.as_ref().unwrap().episode_id,
                    EpisodeAction::Complete {
                        body: "secret completion".into(),
                    },
                )?;
                Ok(TurnOutcome {
                    reply: Some("secret reply".into()),
                    ..done(&request)
                })
            })
        }).await;
    }
    hive(&c, "work", &["a", "b", "outsider"]).await;
    let mut initial = message("secret input", Destination::Hive("work".into()));
    initial.only_for = vec!["b".into()];
    c.send(initial).await.unwrap();
    c.run_until_idle().await.unwrap();
    let reopened =
        Coordinator::new("reopened".into(), storage, CoordinatorOptions::default()).await.unwrap();
    for c in [&c, &reopened] {
        for participant in ["a", "b"] {
            let rows = c.read_hive(participant, "work", None, None).unwrap();
            assert!(rows.iter().any(|row| row.body == "secret post"));
            assert!(rows.iter().any(|row| row.body == "secret completion"));
            assert!(rows.iter().any(|row| row.body == "secret reply"));
        }
        assert_eq!(
            c.read_hive("outsider", "work", None, None).unwrap(),
            Vec::<Message>::new()
        );
    }
}
#[tokio::test]
async fn initiating_private_audience_covers_ordinary_outputs() {
    private_initial_contract(Arc::new(MemoryStorage::new())).await;
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn initiating_private_audience_survives_sqlite_reopen() {
    let directory = tempfile::tempdir().unwrap();
    private_initial_contract(Arc::new(
        crate::SqliteStorage::open(directory.path().join("privacy.sqlite")).unwrap(),
    ))
    .await;
}

async fn direct_reply_contract(storage: Arc<dyn crate::Storage>) {
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    ).await
    .unwrap();
    for id in ["a", "b", "outsider"] {
        add(&c, id, |request| {
            Box::pin(async move {
                Ok(TurnOutcome {
                    reply: Some("answer".into()),
                    ..done(&request)
                })
            })
        }).await;
    }
    let receipt = c
        .send(message("question", Destination::Agent("b".into()))).await
        .unwrap();
    assert_eq!(c.run_until_idle().await.unwrap().completed, 1);
    assert_eq!(c.run_until_idle().await.unwrap().completed, 0);
    let reopened =
        Coordinator::new("reopened".into(), storage, CoordinatorOptions::default()).await.unwrap();
    for c in [&c, &reopened] {
        for (actor, peer) in [("a", "b"), ("b", "a")] {
            let rows = c.read_direct(actor, peer, None).unwrap();
            assert_eq!(
                rows.iter().map(|row| row.body.as_str()).collect::<Vec<_>>(),
                ["question", "answer"]
            );
            let replies = c.read_direct(actor, peer, Some(receipt.sequence)).unwrap();
            assert_eq!(replies.len(), 1);
            assert_eq!(replies[0].sender, "b");
            assert_eq!(
                c.read_direct(actor, peer, Some(replies[0].sequence))
                    .unwrap(),
                Vec::<Message>::new()
            );
        }
        assert_eq!(
            c.read_direct("outsider", "b", None).unwrap(),
            Vec::<Message>::new()
        );
        assert!(matches!(
            c.read_direct("missing", "b", None),
            Err(crate::Error::UnknownAgent(_))
        ));
        assert!(matches!(
            c.read_direct("a", "missing", None),
            Err(crate::Error::UnknownAgent(_))
        ));
    }
}
#[tokio::test]
async fn direct_reply_is_observable_without_an_automatic_return_turn() {
    direct_reply_contract(Arc::new(MemoryStorage::new())).await;
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn direct_reply_transcript_survives_sqlite_reopen() {
    let directory = tempfile::tempdir().unwrap();
    direct_reply_contract(Arc::new(
        crate::SqliteStorage::open(directory.path().join("direct.sqlite")).unwrap(),
    ))
    .await;
}
