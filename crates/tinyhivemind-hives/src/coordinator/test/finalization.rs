//! Failed finalization retains valid sessions but commits no episode outputs.
use super::*;
async fn failed_actions_and_session_contract(
    storage: Arc<dyn Storage>,
    reopen: impl FnOnce() -> Arc<dyn Storage>,
) {
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    ).await
    .unwrap();
    let actions = Arc::downgrade(&c.inner);
    add(&c, "a", move |request| {
        let c = Coordinator {
            inner: actions.upgrade().unwrap(),
        };
        Box::pin(async move {
            let episode = request.episode.as_ref().unwrap();
            c.submit_action(
                "a",
                &episode.episode_id,
                EpisodeAction::Post {
                    body: "SUPPRESSED_POST".into(),
                },
            )?;
            c.submit_action(
                "a",
                &episode.episode_id,
                EpisodeAction::Complete {
                    body: "SUPPRESSED_COMPLETION".into(),
                },
            )?;
            Ok(TurnOutcome {
                session_id: "committed-host-session".into(),
                reply: Some("SUPPRESSED_REPLY".into()),
                disposition: TurnDisposition::Failed("host finalization failed".into()),
            })
        })
    }).await;
    hive(&c, "work", &["a"]).await;
    c.send_as_host(message("first", Destination::Hive("work".into()))).await
        .unwrap();
    assert_eq!(c.run_until_idle().await.unwrap().failed, 1);
    let failed = storage.load().await.unwrap();
    assert_eq!(
        failed.agents["a"].session_id.as_deref(),
        Some("committed-host-session")
    );
    assert!(
        !failed
            .messages
            .iter()
            .any(|message| message.body.starts_with("SUPPRESSED_"))
    );
    assert_eq!(failed.interruptions.len(), 1);
    assert_eq!(failed.running.len(), 0);
    assert!(failed.episodes[0].finished);
    drop(c);
    drop(storage);
    let resumed =
        Coordinator::new("runtime".into(), reopen(), CoordinatorOptions::default()).await.unwrap();
    add(&resumed, "a", |request| {
        Box::pin(async move {
            assert_eq!(
                request.session_id.as_deref(),
                Some("committed-host-session")
            );
            Ok(done(&request))
        })
    }).await;
    resumed
        .send_as_host(message("next", Destination::Agent("a".into()))).await
        .unwrap();
    assert_eq!(resumed.run_until_idle().await.unwrap().completed, 1);
    assert_eq!(resumed.interruptions().unwrap().len(), 1);
}
#[tokio::test]
async fn failed_finalization_retains_session_and_suppresses_actions_in_memory() {
    let storage = Arc::new(MemoryStorage::new());
    let restored = storage.clone();
    failed_actions_and_session_contract(storage, move || restored).await;
}
#[tokio::test]
#[cfg(feature = "sqlite")]
async fn failed_finalization_session_and_suppression_survive_sqlite_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("finalization.sqlite");
    let restored_path = path.clone();
    failed_actions_and_session_contract(
        Arc::new(crate::SqliteStorage::open(&path).unwrap()),
        move || Arc::new(crate::SqliteStorage::open(restored_path).unwrap()),
    )
    .await;
    let reopened = crate::SqliteStorage::open(&path).unwrap();
    assert_eq!(
        reopened.load().await.unwrap().agents["a"].session_id.as_deref(),
        Some("committed-host-session")
    );
    assert_eq!(reopened.load().await.unwrap().interruptions.len(), 1);
}
#[tokio::test]
async fn failed_outcomes_with_empty_or_changed_sessions_do_not_replace_a_binding() {
    for (original, returned) in [(None, ""), (Some("existing"), "changed")] {
        let c = setup().await;
        add(&c, "a", move |_| {
            Box::pin(async move {
                Ok(TurnOutcome {
                    session_id: returned.into(),
                    reply: None,
                    disposition: TurnDisposition::Failed("host finalization failed".into()),
                })
            })
        }).await;
        if let Some(session) = original {
            c.bind_session("a", session).await.unwrap();
        }
        c.send_as_host(message("first", Destination::Agent("a".into()))).await
            .unwrap();
        assert_eq!(c.run_until_idle().await.unwrap().failed, 1);
        assert_eq!(
            c.lock().unwrap().durable.agents["a"].session_id.as_deref(),
            original
        );
        assert_eq!(c.interruptions().unwrap().len(), 1);
    }
}
