//! Incremental commits outside the live lock, conflict reload, and retention.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{Commit, DeliveryStatus, RetentionPolicy, Storage, StorageFuture, StoredState};
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
async fn second_coordinator_fences_first_coordinator_from_writing() {
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

    // A second process claims ownership by starting a new coordinator.
    // This increments writer_epoch, fencing the first coordinator.
    let other = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();

    // The first coordinator is now fenced. Any attempt to write returns Fenced.
    assert!(matches!(
        c.send_as_host(message("fenced", Destination::Agent("a".into())))
            .await,
        Err(Error::Fenced { coordinator: 1, stored: 2 })
    ));

    // The second coordinator can create a hive and operate normally.
    hive(&other, "elsewhere", &["a"]).await;
    assert_eq!(other.list_hives().unwrap()[0].hive_id, "elsewhere");

    let stored = storage.load().await.unwrap();
    // The hive was created, but the fenced message was never sent.
    assert!(stored.hives.contains_key("elsewhere"));
    assert_eq!(stored.messages.len(), 0);
}
#[tokio::test]
async fn second_coordinator_recovers_interrupted_turns_on_restart() {
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

    // Send a message and cancel the turn mid-execution.
    c.send_as_host(message("task", Destination::Agent("a".into())))
        .await
        .unwrap();
    let mut drain = Box::pin(c.run_until_idle());
    tokio::select! { () = started.notified() => {}, result = &mut drain => { assert!(result.is_err()); } }
    drop(drain);

    // The turn is running and there's an unpersisted interruption.
    assert_eq!(c.interruptions().unwrap().len(), 1);

    // A second coordinator starts up and claims ownership.
    // During startup, it should recover the interrupted running turn.
    let other = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();

    // The second coordinator sees the recovered interruption.
    assert_eq!(other.interruptions().unwrap().len(), 1);

    // The first coordinator is now fenced and cannot make further progress.
    assert!(matches!(
        c.send_as_host(message("blocked", Destination::Agent("a".into())))
            .await,
        Err(Error::Fenced { .. })
    ));
}

#[tokio::test]
async fn conflicts_and_storage_failures_are_fatal() {
    let storage = Arc::new(Recording::default());
    let c = over(storage.clone(), CoordinatorOptions::default()).await;
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;

    // With single-writer fencing, any conflict is fatal (not retried).
    storage.conflicts.store(1, Ordering::SeqCst);
    assert!(matches!(
        c.send_as_host(message("conflict", Destination::Agent("a".into())))
            .await,
        Err(Error::RevisionConflict { .. })
    ));
    assert_eq!(c.lock().unwrap().durable.messages.len(), 0);
    assert_eq!(storage.load().await.unwrap().messages.len(), 0);

    // Storage failures are also fatal.
    storage.fail_next_commits(1);
    assert!(matches!(
        c.send_as_host(message("offline", Destination::Agent("a".into())))
            .await,
        Err(Error::InvalidState(_))
    ));
    assert_eq!(storage.load().await.unwrap().messages.len(), 0);
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
    assert_eq!(storage.load().await.unwrap().interruptions.len(), 0);
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
                interrupted: None,
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
#[tokio::test]
async fn deferred_interruptions_only_reapply_to_matching_reservations() {
    // Regression test for P1: deferred interruptions must match the reservation
    // they were intended to interrupt, not just the agent ID.
    let storage = Arc::new(Recording::default());
    let c = over(storage.clone(), CoordinatorOptions::default()).await;
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    add(&c, "a", move |_request| {
        let signal = signal.clone();
        Box::pin(async move {
            signal.notify_one();
            std::future::pending().await
        })
    })
    .await;
    // Send a direct message and cancel the turn mid-execution.
    c.send_as_host(message("first", Destination::Agent("a".into())))
        .await
        .unwrap();
    let mut drain = Box::pin(c.run_until_idle());
    tokio::select! { () = started.notified() => {}, result = &mut drain => { assert!(result.is_err()); } }
    drop(drain);
    assert_eq!(c.interruptions().unwrap().len(), 1);
    // Another coordinator claims the agent's running delivery and starts newer work.
    let other = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    // Reload to get the now-interrupted delivery.
    other.run_until_idle().await.unwrap();
    assert_eq!(other.interruptions().unwrap().len(), 1);
    // Send a new message for the agent; the other coordinator processes it.
    other
        .send_as_host(message("second", Destination::Agent("a".into())))
        .await
        .unwrap();
    other.run_until_idle().await.unwrap();
    // Now back in the original coordinator, a commit that includes the deferred
    // interruption from "first" encounters a conflict (another process has updated
    // the store). The coordinator reloads and recomputes. The deferred interruption
    // must not be reapplied to "second" just because they are the same agent.
    hive(&c, "work", &["a"]).await;
    c.run_until_idle().await.unwrap();
    // Both interruptions should be in the history: the first is from the cancelled
    // turn, the second is still running or queued.
    let stored = storage.load().await.unwrap();
    // The key assertion: we should have one interruption from the first cancelled
    // turn, not two (which would have happened if the deferred interruption was
    // incorrectly reapplied to the second message's turn).
    assert_eq!(stored.interruptions.len(), 1);
    assert_eq!(stored.interruptions[0].message_ids[0], "first");
}
#[tokio::test]
async fn retention_bounds_interrupted_records() {
    // Regression test for P2: retention policy must also prune interrupted
    // deliveries and interruption records, not just delivered ones.
    let storage = Arc::new(Recording::default());
    let c = over(
        storage.clone(),
        CoordinatorOptions {
            retention: RetentionPolicy {
                settled_episodes: Some(1),
                delivered: Some(1),
                interrupted: Some(1),
            },
            ..CoordinatorOptions::default()
        },
    )
    .await;
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    // Create multiple interruptions by sending direct messages and cancelling turns.
    for i in 1..=3 {
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
        c.send_as_host(message(&format!("msg-{i}"), Destination::Agent("a".into())))
            .await
            .unwrap();
        let mut drain = Box::pin(c.run_until_idle());
        tokio::select! { () = started.notified() => {}, result = &mut drain => { assert!(result.is_err()); } }
        drop(drain);
    }
    assert_eq!(c.interruptions().unwrap().len(), 3);
    // Trigger a commit to apply retention.
    hive(&c, "work", &["a"]).await;
    c.run_until_idle().await.unwrap();
    let stored = storage.load().await.unwrap();
    // With interrupted=Some(1), only the most recent interruption should be kept.
    assert_eq!(stored.interruptions.len(), 1);
    assert_eq!(stored.interruptions[0].message_ids[0], "msg-3");
    // Also check that interrupted deliveries were pruned.
    let interrupted_deliveries = stored
        .deliveries
        .iter()
        .filter(|d| d.status == DeliveryStatus::Interrupted)
        .count();
    assert_eq!(interrupted_deliveries, 1);
}
