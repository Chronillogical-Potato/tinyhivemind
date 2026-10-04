//! Host transcript reads, committed-change signals and episode status.
#![allow(clippy::unwrap_used)]
use super::*;

#[tokio::test]
async fn host_reads_its_replies_and_private_rows_in_sequence_order() {
    let c = setup().await;
    add(&c, "a", |request| {
        Box::pin(async move {
            let mut outcome = done(&request);
            outcome.reply = Some("reply to host".into());
            Ok(outcome)
        })
    })
    .await;
    add(&c, "b", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    // Agent reads reject the host identity, so the host needs its own scope.
    assert!(c.read_direct(HOST_ID, "a", None).is_err());
    let asked = c
        .send_as_host(message("question", Destination::Agent("a".into())))
        .await
        .unwrap();
    hive(&c, "work", &["a", "b"]).await;
    let mut private = message("private", Destination::Hive("work".into()));
    private.only_for = vec!["b".into()];
    c.send(private).await.unwrap();
    c.run_until_idle().await.unwrap();
    let rows = c.read_transcript(None).unwrap();
    assert!(
        rows.windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    let reply = rows.iter().find(|row| row.body == "reply to host").unwrap();
    assert_eq!(reply.sender, "a");
    assert_eq!(reply.destination, Destination::Agent(HOST_ID.into()));
    assert!(rows.iter().any(|row| row.message_id == "private"));
    let after = c.read_transcript(Some(asked.sequence)).unwrap();
    assert!(after.iter().all(|row| row.sequence > asked.sequence));
    assert!(after.iter().any(|row| row.body == "reply to host"));
    let last = rows.last().unwrap().sequence;
    assert_eq!(c.read_transcript(Some(last)).unwrap().len(), 0);
}
#[tokio::test]
async fn subscribers_wake_on_every_committed_change() {
    let c = setup().await;
    let mut revisions = c.subscribe();
    let start = *revisions.borrow_and_update();
    add(&c, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    })
    .await;
    assert!(revisions.has_changed().unwrap());
    assert_eq!(*revisions.borrow_and_update(), start + 1);
    let waiter = tokio::spawn(async move {
        revisions.changed().await.unwrap();
        *revisions.borrow()
    });
    c.send_as_host(message("wake", Destination::Agent("a".into())))
        .await
        .unwrap();
    assert_eq!(waiter.await.unwrap(), start + 2);
    // Reads commit nothing and signal nothing.
    let mut quiet = c.subscribe();
    quiet.borrow_and_update();
    c.read_transcript(None).unwrap();
    assert!(!quiet.has_changed().unwrap());
}
#[tokio::test]
async fn episode_snapshot_reports_open_waiting_settled_and_failed() {
    let c = setup().await;
    for (agent, hive_id) in [("a", "parks"), ("b", "finishes"), ("c", "stalls")] {
        let actions = c.clone();
        add(&c, agent, move |request| {
            let c = actions.clone();
            Box::pin(async move {
                let episode = request.episode.clone().unwrap();
                let mut outcome = done(&request);
                match request.agent_id.as_str() {
                    "a" => outcome.disposition = TurnDisposition::Parked,
                    "b" => {
                        c.submit_action(
                            "b",
                            &episode.episode_id,
                            EpisodeAction::Complete {
                                body: "done".into(),
                            },
                        )
                        .await?;
                    }
                    _ => {}
                }
                Ok(outcome)
            })
        })
        .await;
        hive(&c, hive_id, &[agent]).await;
        let mut task = message(hive_id, Destination::Hive(hive_id.into()));
        task.sender = agent.into();
        c.send(task).await.unwrap();
    }
    let opened = c.episodes().unwrap();
    assert_eq!(opened.len(), 3);
    assert!(opened.iter().all(|e| e.phase == EpisodePhase::Open));
    assert_eq!(opened[0].hive_id, "parks");
    assert_eq!(opened[0].starters, ["a"]);
    c.run_until_idle().await.unwrap();
    let phases: Vec<_> = c.episodes().unwrap().into_iter().map(|e| e.phase).collect();
    assert_eq!(phases[0], EpisodePhase::AwaitingRelease);
    assert_eq!(phases[1], EpisodePhase::Settled);
    assert!(matches!(&phases[2], EpisodePhase::Failed(reason) if reason.contains("stalled")));
}
