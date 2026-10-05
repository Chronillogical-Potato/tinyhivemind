//! Releasing a parked agent with a host note that rides on its next turn.
#![allow(clippy::unwrap_used)]
use super::*;

async fn parks_once(c: &Coordinator) -> Arc<std::sync::Mutex<Vec<TurnRequest>>> {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = seen.clone();
    add(c, "a", move |request| {
        let recorded = recorded.clone();
        Box::pin(async move {
            let first = recorded.lock().unwrap().is_empty();
            recorded.lock().unwrap().push(request.clone());
            let mut outcome = done(&request);
            if first {
                outcome.disposition = TurnDisposition::Parked;
            }
            Ok(outcome)
        })
    })
    .await;
    seen
}
#[tokio::test]
async fn a_release_note_rides_on_the_next_claimed_turn_only() {
    let c = setup().await;
    let seen = parks_once(&c).await;
    c.send_as_host(message("task", Destination::Agent("a".into())))
        .await
        .unwrap();
    assert_eq!(c.run_until_idle().await.unwrap().parked, 1);
    c.release_with("a", Some("approved: deploy to staging only".into()))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    c.send_as_host(message("later", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0].resumption, None);
    assert_eq!(
        seen[1].resumption.as_deref(),
        Some("approved: deploy to staging only")
    );
    assert_eq!(seen[2].resumption, None);
}
#[tokio::test]
async fn release_without_a_note_resumes_as_before_and_notes_survive_restart() {
    let storage = Arc::new(MemoryStorage::new());
    let c = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .await
    .unwrap();
    let seen = parks_once(&c).await;
    c.send_as_host(message("task", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.run_until_idle().await.unwrap();
    c.release("a").await.unwrap();
    c.run_until_idle().await.unwrap();
    assert_eq!(seen.lock().unwrap()[1].resumption, None);
    assert!(c.release_with("missing", Some("x".into())).await.is_err());
    // A note recorded before a restart reaches the reattached agent.
    c.send_as_host(message("again", Destination::Agent("a".into())))
        .await
        .unwrap();
    c.release_with("a", Some("carried over".into()))
        .await
        .unwrap();
    drop(c);
    let restored = Coordinator::new("runtime".into(), storage, CoordinatorOptions::default())
        .await
        .unwrap();
    let seen = parks_once(&restored).await;
    restored.run_until_idle().await.unwrap();
    assert_eq!(
        seen.lock().unwrap()[0].resumption.as_deref(),
        Some("carried over")
    );
}
#[test]
fn turn_requests_without_a_note_keep_their_wire_form() {
    let request = TurnRequest {
        agent_id: "a".into(),
        session_id: None,
        messages: Vec::new(),
        memberships: Vec::new(),
        episode: None,
        resumption: None,
    };
    let json = serde_json::to_value(&request).unwrap();
    assert!(json.get("resumption").is_none());
    assert_eq!(
        serde_json::from_value::<TurnRequest>(json).unwrap(),
        request
    );
}
