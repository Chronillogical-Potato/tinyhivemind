//! The seam: which runner the environment names, and both runners through it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use openhuman_embed::{Access, Provider, Runtime, Workspace};
use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind_driver::standing_contract;
use tinyhivemind_tools::{Dispatch, EpisodeTools, SeatEvent, served_specs};

use super::{Lane, RunnerKind, SeatRunner};
use crate::{EmbedRunner, RawRunner, Route, offline};

#[test]
fn the_runner_is_named_by_the_environment_and_defaults_to_embed() {
    // Not set, empty, and each spelling -- read through the same parser the
    // binary uses, without touching the process environment.
    assert_eq!(RunnerKind::parse(None), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("")), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("embed")), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("raw")), Ok(RunnerKind::Raw));
    assert!(
        RunnerKind::parse(Some("rae")).is_err(),
        "a typo must not run the default"
    );
    // The environment, whatever it holds, reads through the same parser.
    let from_env = RunnerKind::from_env();
    let value = std::env::var("TINYHIVEMIND_RUNNER").ok();
    assert_eq!(
        from_env.map_err(|error| error.to_string()),
        RunnerKind::parse(value.as_deref()).map_err(|other| format!(
            "TINYHIVEMIND_RUNNER must be `embed` or `raw`, not `{other}`"
        ))
    );
}

#[test]
fn each_runner_states_its_own_mechanics_and_nothing_else() {
    assert!(RunnerKind::Embed.how_to_call().contains("mcp_call_tool"));
    assert!(!RunnerKind::Raw.how_to_call().contains("mcp"));
    assert_eq!(RunnerKind::Embed.name(), "embed");
    assert_eq!(RunnerKind::Raw.name(), "raw");
}

/// One turn through the seam: open, run, close.
async fn one_turn<R: SeatRunner>(runner: &R) -> (String, Vec<SeatEvent>) {
    let bindings = runner.bindings();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].hive_agent_id, "lead");
    assert!(!bindings[0].runtime_agent_id().is_empty());
    runner.open(
        "lead",
        vec!["    1  @operator: state the root cause".into()],
        Dispatch {
            chat: "engineering".into(),
            parent: None,
        },
    );
    let (seat, lane, reply) = runner
        .turn("lead".into(), Lane::Desk, "Your turn.".into())
        .await;
    assert_eq!(seat, "lead");
    assert_eq!(lane, Lane::Desk);
    let reply = reply
        .expect("the turn did not time out")
        .expect("the turn ran");
    (reply, runner.close("lead"))
}

/// Both runners, offline, against one scripted model: the same call lands in
/// the record the same way, whichever road it took. One test rather than
/// two because the runtime and the definition registry are process-wide, and
/// the raw seats must be registered before the runtime boots.
#[tokio::test(flavor = "multi_thread")]
async fn both_runners_land_the_same_scripted_call_in_the_record() {
    let workspace = tempfile::tempdir().expect("a workspace");
    let metrics = Arc::new(offline::Metrics::default());
    let model = offline::model("engineering", Arc::clone(&metrics)).await;
    let backend = offline::backend().await;
    let route = Route {
        endpoint: format!("{}/v1", model.uri()),
        api_key: "local-test-key".into(),
        model: offline::MODEL.into(),
    };
    let mut config = offline::config();
    config.agent.max_tool_iterations = 6;
    config.default_temperature = 0.0;
    let briefs: BTreeMap<String, String> = [("lead".to_owned(), "You lead the desk.".to_owned())]
        .into_iter()
        .collect();
    let contract =
        |kind: RunnerKind| standing_contract(served_specs(), "engineering", kind.how_to_call());

    RawRunner::prepare(workspace.path(), &[("lead", "You lead the desk.")])
        .expect("seats register");
    let runtime = Box::pin(
        Runtime::builder()
            .config(config.clone())
            .workspace(Workspace::dir(workspace.path().to_path_buf()))
            .services(EmbedRunner::services())
            .backend_url(backend.uri())
            .provider(
                Provider::openai_compatible(route.endpoint.clone(), route.api_key.clone())
                    .model(route.model.clone()),
            )
            .access(Access::full())
            .build(),
    )
    .await
    .expect("the runtime boots");
    let embed = EmbedRunner::seat(
        &runtime,
        Arc::new(EpisodeTools::new(["lead"])),
        &briefs,
        &contract(RunnerKind::Embed),
        "test",
    )
    .await
    .expect("embed seats");
    let raw = RawRunner::seat(
        Arc::new(EpisodeTools::new(["lead"])),
        &briefs,
        &contract(RunnerKind::Raw),
        &config,
        &backend.uri(),
        &route,
        workspace.path(),
    )
    .await
    .expect("raw seats");
    assert_eq!(raw.model(), offline::MODEL);
    let shown = format!("{:?}", raw.bindings()[0].agent);
    assert!(
        shown.contains("lead") && shown.contains(offline::MODEL),
        "{shown}"
    );
    assert!(format!("{raw:?}").contains("lead"));
    assert!(format!("{embed:?}").contains("lead"));

    let (embed_reply, embed_events) = one_turn(&embed).await;
    let (raw_reply, raw_events) = one_turn(&raw).await;
    for (name, events) in [("embed", &embed_events), ("raw", &raw_events)] {
        assert_eq!(events.len(), 1, "{name}: one call recorded");
        assert_eq!(events[0].seat, "lead");
        assert_eq!(events[0].dispatch.chat, "engineering");
        assert!(
            matches!(
                &events[0].call,
                ToolCall::Speak(Utterance::CompleteEpisode { message, .. })
                    if message == offline::COMPLETION
            ),
            "{name}: {:?}",
            events[0].call
        );
    }
    assert_eq!(
        embed_reply, raw_reply,
        "the closing sentence is the model's"
    );
    let seen = metrics.snapshot();
    assert_eq!(seen.round_trips.len(), 2, "one receipted call per runner");
    assert!(seen.requests >= 4, "each turn is a call and a receipt");

    // A second raw turn is seeded with the first: what the seat said is what
    // it is shown, and the record starts empty again.
    let (_, again) = one_turn(&raw).await;
    assert_eq!(again.len(), 1);
    metrics.reset();
    assert_eq!(metrics.snapshot().requests, 0);
}
