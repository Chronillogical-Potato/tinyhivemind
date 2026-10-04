//! Host-chosen starters: who opens an episode, separate from who reads it.
#![allow(clippy::unwrap_used)]
use super::*;

#[tokio::test]
async fn starters_open_the_episode_while_every_member_reads_the_message() {
    let c = setup().await;
    let ran = Arc::new(std::sync::Mutex::new(Vec::new()));
    for id in ["a", "b", "c"] {
        let actions = c.clone();
        let ran = ran.clone();
        add(&c, id, move |request| {
            let c = actions.clone();
            let ran = ran.clone();
            Box::pin(async move {
                ran.lock().unwrap().push(request.agent_id.clone());
                c.submit_action(
                    &request.agent_id,
                    &request.episode.as_ref().unwrap().episode_id,
                    EpisodeAction::Complete {
                        body: "done".into(),
                    },
                )
                .await?;
                Ok(done(&request))
            })
        })
        .await;
    }
    hive(&c, "work", &["a", "b", "c"]).await;
    let mut task = message("task", Destination::Hive("work".into()));
    task.starters = vec!["b".into()];
    c.send_as_host(task).await.unwrap();
    assert_eq!(c.episodes().unwrap()[0].starters, ["b"]);
    c.run_until_idle().await.unwrap();
    assert_eq!(*ran.lock().unwrap(), ["b"]);
    for reader in ["a", "c"] {
        let rows = c.read_hive(reader, "work", None, None).unwrap();
        assert!(rows.iter().any(|row| row.message_id == "task"));
    }
    assert_eq!(c.read_transcript(None).unwrap()[0].only_for.len(), 0);
}
#[tokio::test]
async fn starters_must_be_distinct_members_who_can_read_the_message() {
    let c = setup().await;
    for id in ["a", "b", "outsider"] {
        add(&c, id, |request| {
            Box::pin(async move { Ok(done(&request)) })
        })
        .await;
    }
    hive(&c, "work", &["a", "b"]).await;
    let cases: [(&[&str], &[&str]); 3] =
        [(&["outsider"], &[]), (&["a", "a"], &[]), (&["a"], &["b"])];
    for (index, (starters, only_for)) in cases.into_iter().enumerate() {
        let mut task = message(&format!("bad-{index}"), Destination::Hive("work".into()));
        task.starters = starters.iter().map(|id| (*id).into()).collect();
        task.only_for = only_for.iter().map(|id| (*id).into()).collect();
        let result = c.send_as_host(task).await;
        assert!(
            matches!(
                result,
                Err(Error::NotMember { .. } | Error::DuplicateMember(_))
            ),
            "{starters:?} with readers {only_for:?} must be refused, got {result:?}"
        );
    }
    let mut direct = message("direct", Destination::Agent("a".into()));
    direct.starters = vec!["a".into()];
    assert!(matches!(
        c.send_as_host(direct).await,
        Err(Error::InvalidIdentifier(_))
    ));
    assert_eq!(c.read_transcript(None).unwrap().len(), 0);
}
#[test]
fn empty_starters_keep_the_previous_wire_form() {
    let plain = message("m", Destination::Hive("work".into()));
    let json = serde_json::to_value(&plain).unwrap();
    assert!(json.get("starters").is_none());
    let mut chosen = plain.clone();
    chosen.starters = vec!["b".into()];
    let json = serde_json::to_value(&chosen).unwrap();
    assert_eq!(json["starters"], serde_json::json!(["b"]));
    let decoded: SendMessage = serde_json::from_value(serde_json::json!({
        "message_id":"m","sender":"a","destination":{"Hive":"work"},
        "body":"m","thread":null,"only_for":[]
    }))
    .unwrap();
    assert_eq!(decoded, plain);
}
