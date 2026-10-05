//! A finalization failure keeps committed `OpenHuman` history for the next delivery.
use super::*;
use std::sync::atomic::Ordering;
use tinyhivemind_hives::{Destination, SendMessage, Storage};
fn input(id: &str) -> SendMessage {
    SendMessage {
        message_id: id.into(),
        sender: String::new(),
        destination: Destination::Agent("continuing".into()),
        body: id.into(),
        thread: None,
        only_for: vec![],
        starters: Vec::new(),
    }
}
#[test]
fn first_turn_finalization_failure_keeps_the_session_and_prior_provider_history() {
    let _guard = RUNTIME_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    executor().block_on(async {tokio::spawn(async {
        let (runtime,_backend,_) = Box::pin(fixture()).await;
        let storage=Arc::new(MemoryStorage::new());
        let coordinator=Coordinator::new(runtime.runtime_id().into(),storage.clone(),CoordinatorOptions::default()).await.unwrap();
        let hooks=Arc::new(Hooks::default());
        hooks.mode.store(2,Ordering::SeqCst);
        let host=OpenHumanHost::new(runtime.runtime_id().into(),coordinator).unwrap().with_hooks(hooks.clone()).unwrap();
        let provider=wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/chat/completions"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id":"fixture","object":"chat.completion","created":0,"model":"fixture",
                "choices":[{"index":0,"message":{"role":"assistant","content":"COMMITTED_REPLY"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}
            }))).mount(&provider).await;
        let agent=runtime.agent(AgentSpec::new("continuing")
            .provider(openhuman_embed::Provider::openai_compatible(format!("{}/v1",provider.uri()),"fixture").model("fixture"))).unwrap();
        host.register_agent(agent).await.unwrap();
        host.coordinator().send_as_host(input("FIRST_COMMITTED_INPUT")).await.unwrap();
        assert_eq!(host.coordinator().run_until_idle().await.unwrap().failed,1);
        let failed=storage.load().await.unwrap();
        let session=failed.agents["continuing"].session_id.clone();
        assert!(session.is_some(), "a completed provider turn must retain its continuing session even when finalization fails");
        let session=session.unwrap();
        assert_eq!(failed.deliveries[0].status,tinyhivemind_hives::DeliveryStatus::Interrupted);
        assert!(!failed.messages.iter().any(|message|message.body=="COMMITTED_REPLY"));
        hooks.mode.store(0,Ordering::SeqCst);
        host.coordinator().send_as_host(input("SECOND_INPUT")).await.unwrap();
        assert_eq!(host.coordinator().run_until_idle().await.unwrap().completed,1);
        assert_eq!(storage.load().await.unwrap().agents["continuing"].session_id.as_deref(),Some(session.as_str()));
        let requests:Vec<_>=provider.received_requests().await.unwrap().into_iter().filter(|request|request.method==wiremock::http::Method::POST&&request.url.path()=="/v1/chat/completions").collect();
        assert_eq!(requests.len(),2);
        let second:serde_json::Value=serde_json::from_slice(&requests[1].body).unwrap();
        let messages=second["messages"].as_array().unwrap();
        assert!(messages.iter().any(|message|message["role"]=="user"&&message["content"].as_str().is_some_and(|body|body.contains("FIRST_COMMITTED_INPUT"))));
        assert!(messages.iter().any(|message|message["role"]=="assistant"&&message["content"]=="COMMITTED_REPLY"));
        assert_eq!(host.coordinator().interruptions().unwrap().len(),1);
    }).await.unwrap();});
}
