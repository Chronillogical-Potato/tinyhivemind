//! Private child transcript visibility and addressed-thread execution regressions.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::Storage;

async fn private_child_contract(storage: Arc<dyn Storage>) {
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    for id in ["a", "b", "c"] {
        let c2 = c.clone();
        add(&c, id, move |request| {
            let c = c2.clone();
            Box::pin(async move {
                let ep = request.episode.as_ref().unwrap();
                if request.agent_id == "a" && request.session_id.is_none() {
                    c.submit_action(
                        "a",
                        &ep.episode_id,
                        EpisodeAction::Ask {
                            agents: vec!["b".into()],
                            body: "private question".into(),
                        },
                    )
                    .await?;
                } else {
                    if ep.thread.is_some() {
                        c.submit_action(
                            &request.agent_id,
                            &ep.episode_id,
                            EpisodeAction::Post {
                                body: "private post".into(),
                            },
                        )
                        .await?;
                    }
                    c.submit_action(
                        &request.agent_id,
                        &ep.episode_id,
                        EpisodeAction::Complete {
                            body: if ep.thread.is_some() {
                                "private completion".into()
                            } else {
                                "done".into()
                            },
                        },
                    )
                    .await?;
                }
                Ok(done(&request))
            })
        })
        .await;
    }
    hive(&c, "work", &["a", "b", "c"]).await;
    let mut task = message("task", Destination::Hive("work".into()));
    task.only_for = vec!["a".into()];
    c.send_as_host(task).await.unwrap();
    c.run_until_idle().await.unwrap();
    for coordinator in [
        &c,
        &Coordinator::new("reopened".into(), storage, CoordinatorOptions::default())
            .await
            .unwrap(),
    ] {
        let outsider = coordinator.read_hive("c", "work", None, None).unwrap();
        assert!(!outsider.iter().any(|row| row.body.starts_with("private")));
        let participant = coordinator.read_hive("a", "work", None, None).unwrap();
        let root = participant
            .iter()
            .find(|row| row.body == "private question")
            .unwrap()
            .sequence;
        assert!(
            coordinator
                .read_hive("c", "work", None, Some(root))
                .is_err()
        );
        let thread = coordinator
            .read_hive("b", "work", None, Some(root))
            .unwrap();
        assert!(thread.iter().any(|row| row.body == "private post"));
        assert!(thread.iter().any(|row| row.body == "private completion"));
        for row in thread.iter().filter(|row| row.thread.is_some()) {
            assert!(row.only_for.contains(&"a".into()) && row.only_for.contains(&"b".into()));
        }
    }
}
#[tokio::test]
async fn private_child_posts_and_completions_are_hidden_from_other_hive_members() {
    private_child_contract(Arc::new(MemoryStorage::new())).await;
}
#[cfg(feature = "sqlite")]
#[tokio::test]
async fn private_child_visibility_survives_sqlite_reopen() {
    let directory = tempfile::tempdir().unwrap();
    private_child_contract(Arc::new(
        crate::SqliteStorage::open(directory.path().join("hives.sqlite")).unwrap(),
    ))
    .await;
}
#[tokio::test]
async fn addressed_private_thread_keeps_turn_context_and_outputs_in_that_thread() {
    let c = setup().await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    for id in ["a", "b", "c"] {
        let c2 = c.clone();
        let seen = seen.clone();
        add(&c, id, move |request| {
            let c = c2.clone();
            let seen = seen.clone();
            Box::pin(async move {
                seen.lock().unwrap().push(request.clone());
                let ep = request.episode.as_ref().unwrap();
                c.submit_action(
                    &request.agent_id,
                    &ep.episode_id,
                    EpisodeAction::Post {
                        body: "thread post".into(),
                    },
                )
                .await?;
                c.submit_action(
                    &request.agent_id,
                    &ep.episode_id,
                    EpisodeAction::Complete {
                        body: "thread done".into(),
                    },
                )
                .await?;
                Ok(done(&request))
            })
        })
        .await;
    }
    hive(&c, "work", &["a", "b", "c"]).await;
    let mut initial = message("private root", Destination::Hive("work".into()));
    initial.only_for = vec!["b".into()];
    let root = c.send(initial).await.unwrap();
    c.run_until_idle().await.unwrap();
    seen.lock().unwrap().clear();
    let mut follow = message("follow", Destination::Hive("work".into()));
    follow.thread = Some(root.sequence);
    c.send(follow).await.unwrap();
    c.run_until_idle().await.unwrap();
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|request| request.agent_id != "c"
            && request.episode.as_ref().unwrap().thread == Some(root.sequence)));
    }
    let thread = c.read_hive("a", "work", None, Some(root.sequence)).unwrap();
    assert!(
        thread
            .iter()
            .any(|row| row.body == "thread post" && row.thread == Some(root.sequence))
    );
    assert!(
        thread
            .iter()
            .any(|row| row.body == "thread done" && row.thread == Some(root.sequence))
    );
    assert!(
        !c.read_hive("c", "work", None, None)
            .unwrap()
            .iter()
            .any(|row| row.message_id == "follow")
    );
    let mut forbidden = message("forbidden", Destination::Hive("work".into()));
    forbidden.sender = "c".into();
    forbidden.thread = Some(root.sequence);
    assert!(c.send(forbidden).await.is_err());
    let mut widening = message("widening", Destination::Hive("work".into()));
    widening.thread = Some(root.sequence);
    widening.only_for = vec!["c".into()];
    assert!(c.send(widening).await.is_err());
}
