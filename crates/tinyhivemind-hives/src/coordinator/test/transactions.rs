//! Incremental commits outside the live lock, conflict reload, and retention.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{Commit, RetentionPolicy, Storage, StorageFuture, StoredState};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Memory storage that records each commit's appended rows and can be told
/// to fail the next commits, with a conflict or a plain storage error.
#[derive(Default)]
pub(super) struct Recording {
    inner: MemoryStorage,
    appended: std::sync::Mutex<Vec<usize>>,
    conflicts: AtomicUsize,
    failures: AtomicUsize,
}
impl Recording {
    pub(super) fn fail_next_commits(&self, count: usize) {
        self.failures.store(count, Ordering::SeqCst);
    }
}
impl Storage for Recording {
    fn load(&self) -> StorageFuture<'_, StoredState> {
        self.inner.load()
    }
    fn commit<'a>(&'a self, commit: Commit<'a>) -> StorageFuture<'a, ()> {
        if self.failures.load(Ordering::SeqCst) > 0 {
            self.failures.fetch_sub(1, Ordering::SeqCst);
            return Box::pin(async { Err(Error::InvalidState("store offline".into())) });
        }
        if self.conflicts.load(Ordering::SeqCst) > 0 {
            self.conflicts.fetch_sub(1, Ordering::SeqCst);
            return Box::pin(async {
                Err(Error::RevisionConflict {
                    expected: 0,
                    actual: 1,
                })
            });
        }
        self.appended.lock().unwrap().push(commit.appended.len());
        self.inner.commit(commit)
    }
}
async fn over(storage: Arc<Recording>, options: CoordinatorOptions) -> Coordinator {
    Coordinator::new("runtime".into(), storage, options)
        .await
        .unwrap()
}
#[tokio::test]
async fn commits_append_only_new_transcript_rows() {
    let storage = Arc::new(Recording::default());
    let c = over(storage.clone(), CoordinatorOptions::default()).await;
    add(&c, "a", |request| {
        Box::pin(async move {
            let mut outcome = done(&request);
            outcome.reply = Some("reply".into());
            Ok(outcome)
        })
    })
    .await;
    storage.appended.lock().unwrap().clear();
    c.send_as_host(message("one", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    // Acceptance appends one row, the claim none, the reply one.
    assert_eq!(*storage.appended.lock().unwrap(), [1, 0, 1]);
    let stored = storage.load().await.unwrap();
    assert_eq!(stored.messages.len(), 2);
    assert!(stored.accepted.contains_key("one"));
}
#[tokio::test]
async fn a_write_racing_another_process_reloads_and_retries() {
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
    // Another process sharing the store creates a hive behind our back.
    let other = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    hive(&other, "elsewhere", &["a"]).await;
    c.send_as_host(message("raced", Destination::Agent("a".into())))
        .await
        .unwrap();
    assert_eq!(c.lock().unwrap().durable.revision, 3);
    assert_eq!(c.list_hives().unwrap()[0].hive_id, "elsewhere");
    let stored = storage.load().await.unwrap();
    assert_eq!(stored.messages.len(), 1);
    assert!(stored.hives.contains_key("elsewhere"));
}
#[tokio::test]
async fn persistent_conflicts_and_storage_failures_publish_nothing() {
    let storage = Arc::new(Recording::default());
    let c = over(storage.clone(), CoordinatorOptions::default()).await;
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    storage.conflicts.store(usize::MAX, Ordering::SeqCst);
    assert!(matches!(
        c.send_as_host(message("never", Destination::Agent("a".into())))
            .await,
        Err(Error::RevisionConflict { .. })
    ));
    storage.conflicts.store(0, Ordering::SeqCst);
    storage.fail_next_commits(1);
    assert!(matches!(
        c.send_as_host(message("offline", Destination::Agent("a".into())))
            .await,
        Err(Error::InvalidState(_))
    ));
    assert!(c.lock().unwrap().durable.messages.is_empty());
    assert!(storage.load().await.unwrap().messages.is_empty());
    // A bounded number of conflicts is absorbed by reload and retry.
    storage.conflicts.store(2, Ordering::SeqCst);
    c.send_as_host(message("eventually", Destination::Agent("a".into())))
        .await
        .unwrap();
    assert_eq!(storage.load().await.unwrap().messages.len(), 1);
}
#[tokio::test]
async fn a_cancelled_turn_is_persisted_by_the_next_drain() {
    let storage = Arc::new(Recording::default());
    let c = over(storage.clone(), CoordinatorOptions::default()).await;
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
    c.send_as_host(message("cancelled", Destination::Agent("a".into())))
        .await
        .unwrap();
    let mut drain = Box::pin(c.run_until_idle());
    tokio::select! { () = started.notified() => {}, result = &mut drain => { assert!(result.is_err()); } }
    drop(drain);
    assert_eq!(c.interruptions().unwrap().len(), 1);
    assert!(storage.load().await.unwrap().interruptions.is_empty());
    // A commit computed from a base without the interruption keeps it live.
    hive(&c, "work", &["a"]).await;
    assert_eq!(c.interruptions().unwrap().len(), 1);
    c.run_until_idle().await.unwrap();
    let stored = storage.load().await.unwrap();
    assert_eq!(stored.interruptions.len(), 1);
    assert!(stored.running.is_empty());
    assert_eq!(c.interruptions().unwrap().len(), 1);
}
#[tokio::test]
async fn retention_bounds_settled_episodes_and_acknowledged_deliveries() {
    let storage = Arc::new(Recording::default());
    let c = over(
        storage.clone(),
        CoordinatorOptions {
            retention: RetentionPolicy {
                settled_episodes: Some(1),
                delivered: Some(1),
            },
            ..CoordinatorOptions::default()
        },
    )
    .await;
    let actions = c.clone();
    add(&c, "a", move |request| {
        let c = actions.clone();
        Box::pin(async move {
            if let Some(episode) = &request.episode {
                c.submit_action(
                    "a",
                    &episode.episode_id,
                    EpisodeAction::Complete {
                        body: "done".into(),
                    },
                )
                .await?;
            }
            Ok(done(&request))
        })
    })
    .await;
    hive(&c, "work", &["a"]).await;
    for id in ["one", "two", "three"] {
        c.send_as_host(message(id, Destination::Hive("work".into())))
            .await
            .unwrap();
        c.send_as_host(message(
            &format!("direct-{id}"),
            Destination::Agent("a".into()),
        ))
        .await
        .unwrap();
    }
    c.run_until_idle().await.unwrap();
    let stored = storage.load().await.unwrap();
    assert_eq!(stored.episodes.len(), 1);
    assert_eq!(stored.episodes[0].episode_id, "episode:4");
    assert_eq!(stored.deliveries.len(), 1);
    // The transcript itself is never pruned.
    assert!(stored.messages.len() >= 6);
}
