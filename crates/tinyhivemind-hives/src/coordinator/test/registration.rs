//! Atomic handle publication with host-owned continuing sessions.
use super::*;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reattached_pending_work_observes_bound_session_while_scheduler_is_live() {
    let storage = Arc::new(MemoryStorage::new());
    let original = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .unwrap();
    add(&original, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    });
    original
        .send_as_host(message("pending", Destination::Agent("a".into())))
        .unwrap();
    drop(original);
    let restored =
        Coordinator::new("runtime".into(), storage, CoordinatorOptions::default()).unwrap();
    let (seen, mut received) = tokio::sync::mpsc::channel(1);
    let runner: Arc<dyn AgentRunner> = Arc::new(Script(Arc::new(move |request| {
        let seen = seen.clone();
        Box::pin(async move {
            seen.send(request.session_id.clone()).await.unwrap();
            Ok(done(&request))
        })
    })));
    let scheduler = restored.clone();
    let running = tokio::spawn(async move { scheduler.run().await });
    tokio::task::yield_now().await;
    restored
        .register_agent_in_session(
            AgentRegistration {
                agent_id: "a".into(),
                runtime_id: "runtime".into(),
                runner: runner.clone(),
            },
            "host-conversation",
        )
        .unwrap();
    assert_eq!(
        received.recv().await.unwrap().as_deref(),
        Some("host-conversation")
    );
    restored.shutdown();
    running.await.unwrap().unwrap();
    restored
        .register_agent_in_session(
            AgentRegistration {
                agent_id: "a".into(),
                runtime_id: "runtime".into(),
                runner: runner.clone(),
            },
            "host-conversation",
        )
        .unwrap();
    assert!(matches!(
        restored.register_agent_in_session(
            AgentRegistration {
                agent_id: "a".into(),
                runtime_id: "runtime".into(),
                runner
            },
            "replacement"
        ),
        Err(Error::SessionConflict(_))
    ));
}
#[test]
fn session_registration_validates_before_publishing_and_storage_failure_is_atomic() {
    let storage = Arc::new(MemoryStorage::new());
    let writer = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .unwrap();
    add(&writer, "a", |request| {
        Box::pin(async move { Ok(done(&request)) })
    });
    let stale = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .unwrap();
    hive(&writer, "revision", &[]);
    let runner: Arc<dyn AgentRunner> = Arc::new(Script(Arc::new(|request| {
        Box::pin(async move { Ok(done(&request)) })
    })));
    let registration = AgentRegistration {
        agent_id: "a".into(),
        runtime_id: "runtime".into(),
        runner: runner.clone(),
    };
    assert!(matches!(
        stale.register_agent_in_session(registration.clone(), "existing"),
        Err(Error::RevisionConflict { .. })
    ));
    assert_eq!(stale.lock().unwrap().durable.agents["a"].session_id, None);
    assert!(!stale.lock().unwrap().runners.contains_key("a"));
    assert_eq!(storage.load().unwrap().agents["a"].session_id, None);
    let current = Coordinator::new(
        "runtime".into(),
        storage.clone(),
        CoordinatorOptions::default(),
    )
    .unwrap();
    assert!(
        current
            .register_agent_in_session(registration.clone(), "")
            .is_err()
    );
    let mut foreign = registration.clone();
    foreign.runtime_id = "foreign".into();
    assert!(matches!(
        current.register_agent_in_session(foreign, "existing"),
        Err(Error::RuntimeMismatch)
    ));
    assert!(!current.lock().unwrap().runners.contains_key("a"));
    current.register_agent(registration.clone()).unwrap();
    current
        .register_agent_in_session(registration.clone(), "existing")
        .unwrap();
    let other = AgentRegistration {
        runner: Arc::new(Script(Arc::new(|request| {
            Box::pin(async move { Ok(done(&request)) })
        }))),
        ..registration
    };
    assert!(matches!(
        current.register_agent_in_session(other.clone(), "existing"),
        Err(Error::AgentConflict(_))
    ));
    assert_eq!(
        current.lock().unwrap().durable.agents["a"]
            .session_id
            .as_deref(),
        Some("existing")
    );
    let restored =
        Coordinator::new("runtime".into(), storage, CoordinatorOptions::default()).unwrap();
    assert!(matches!(
        restored.register_agent_in_session(other, "switched"),
        Err(Error::SessionConflict(_))
    ));
    assert!(!restored.lock().unwrap().runners.contains_key("a"));
}
