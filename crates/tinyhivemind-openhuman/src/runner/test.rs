//! The seam: which runner the environment names, and both runners through it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use openhuman_core::agent::OpenHumanSessionHost;
use openhuman_core::agent::tinyagents::host::LastTurnUsage;
use openhuman_embed::{Access, Provider, Runtime, Workspace};
use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind::{SESSION_WINDOW, Sequence, SessionLog};
use tinyhivemind_driver::standing_contract;
use tinyhivemind_tools::{Dispatch, EpisodeTools, SeatEvent, served_specs};

use super::{Lane, RunnerKind, SeatRunner};
use crate::MemoryLog;
use crate::{
    EmbedRunner, EpisodeBelt, EpisodeHost, HostedRunner, HostedTurn, Journal, LibraryHost,
    RawRunner, Route, offline, register_seats,
};
use tinyhivemind_driver::{Commit, Note};

/// A host with no agents of its own: its seats are library sessions, its
/// log is in memory, and its wrapper is the core context a library session
/// needs -- which is exactly what a real host installs there.
struct TestHost {
    log: MemoryLog,
    library: LibraryHost,
    prompt: String,
    wrapped: AtomicUsize,
    /// Turns the hook saw, and whether any came with usage.
    after: AtomicUsize,
    metered: AtomicBool,
    /// Whether the hook halts the episode on the next turn.
    halt: AtomicBool,
}

impl Journal for TestHost {
    fn log(&self) -> &dyn SessionLog {
        &self.log
    }

    fn commit(&self, commit: &Commit) -> crate::Result<Sequence> {
        Ok(self.log.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            commit.only_for.as_deref(),
        ))
    }

    fn note(&self, note: &Note) -> crate::Result<()> {
        self.log
            .append("desk", &note.body, note.thread, note.only_for.as_deref());
        Ok(())
    }
}

impl EpisodeHost for TestHost {
    fn build_seat(&self, seat: &str, belt: EpisodeBelt) -> crate::Result<OpenHumanSessionHost> {
        let policy = belt.admit(None);
        self.library.session(seat, &self.prompt, belt.tools, policy)
    }

    fn wrap_turn<'a>(&'a self, _seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        self.wrapped.fetch_add(1, Ordering::SeqCst);
        Box::pin(self.library.scope(turn))
    }

    fn tool_prefix(&self) -> String {
        "desk_".into()
    }

    fn after_turn(&self, seat: &str, usage: Option<&LastTurnUsage>) -> crate::Result<()> {
        assert_eq!(seat, "lead");
        self.after.fetch_add(1, Ordering::SeqCst);
        if usage.is_some() {
            self.metered.store(true, Ordering::SeqCst);
        }
        if self.halt.swap(false, Ordering::SeqCst) {
            return Err(crate::Error::Harness(anyhow::anyhow!(
                "the desk's budget is spent"
            )));
        }
        Ok(())
    }
}

#[test]
fn the_runner_is_named_by_the_environment_and_defaults_to_embed() {
    // Not set, empty, and each spelling -- read through the same parser the
    // binary uses, without touching the process environment.
    assert_eq!(RunnerKind::parse(None), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("")), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("embed")), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("raw")), Ok(RunnerKind::Raw));
    assert_eq!(RunnerKind::parse(Some("hosted")), Ok(RunnerKind::Hosted));
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
            "TINYHIVEMIND_RUNNER must be `embed`, `raw` or `hosted`, not `{other}`"
        ))
    );
}

#[test]
fn each_runner_states_its_own_mechanics_and_nothing_else() {
    assert!(RunnerKind::Embed.how_to_call().contains("mcp_call_tool"));
    assert!(!RunnerKind::Raw.how_to_call().contains("mcp"));
    assert_eq!(RunnerKind::Embed.name(), "embed");
    assert_eq!(RunnerKind::Raw.name(), "raw");
    assert_eq!(RunnerKind::Hosted.name(), "hosted");
    assert_eq!(
        RunnerKind::Hosted.how_to_call(),
        RunnerKind::Raw.how_to_call()
    );
}

/// One turn through the seam: open, run, close.
async fn one_turn<R: SeatRunner>(runner: &R, since: Sequence) -> (String, Vec<SeatEvent>) {
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
        .turn("lead".into(), Lane::Desk, since, "Your turn.".into())
        .await;
    assert_eq!(seat, "lead");
    assert_eq!(lane, Lane::Desk);
    let reply = reply
        .expect("the turn did not time out")
        .expect("the turn ran");
    (reply, runner.close("lead"))
}

/// A host that overrides nothing it need not: no prefix, no wrapper, no
/// hook. Its seat runs under the process default context the library boot
/// installed, which is what a host with a booted core of its own has.
struct PlainHost {
    log: MemoryLog,
    library: LibraryHost,
}

impl Journal for PlainHost {
    fn log(&self) -> &dyn SessionLog {
        &self.log
    }

    fn commit(&self, commit: &Commit) -> crate::Result<Sequence> {
        Ok(self.log.append(
            &commit.author,
            commit.utterance.message(),
            commit.thread,
            None,
        ))
    }

    fn note(&self, note: &Note) -> crate::Result<()> {
        self.log.append("desk", &note.body, note.thread, None);
        Ok(())
    }
}

impl EpisodeHost for PlainHost {
    fn build_seat(&self, seat: &str, belt: EpisodeBelt) -> crate::Result<OpenHumanSessionHost> {
        let policy = belt.admit(None);
        self.library
            .session(seat, "You lead the desk.", belt.tools, policy)
    }
}

/// A hosted seat on a host that keeps every default, run once on the desk
/// and once in a thread it is not in: the defaults hold, the thread turn is
/// seeded from the thread, and a call outside its thread is refused.
async fn plain(library: LibraryHost, contract: &str) {
    let log = MemoryLog::new("engineering");
    log.append("operator", "state the root cause", None, None);
    let host = Arc::new(PlainHost { log, library });
    let runner = HostedRunner::seat(
        Arc::clone(&host),
        Arc::new(EpisodeTools::new(["lead"])),
        &["lead".to_owned()],
        "engineering",
        "Engineering",
        SESSION_WINDOW,
    )
    .expect("hosted seats");
    let _ = contract;
    let (reply, events) = one_turn(&runner, host.log.latest()).await;
    assert!(!reply.is_empty());
    assert_eq!(
        events.len(),
        1,
        "the bare-named belt is admitted by default"
    );
    runner.open(
        "lead",
        Vec::new(),
        Dispatch {
            chat: "engineering".into(),
            parent: Some("1".into()),
        },
    );
    let (_, lane, outcome) = runner
        .turn(
            "lead".into(),
            Lane::Thread(Sequence(1)),
            Sequence(1),
            "In the thread.".into(),
        )
        .await;
    assert_eq!(lane, Lane::Thread(Sequence(1)));
    assert!(matches!(outcome, Some(Ok(_))), "{outcome:?}");
    assert!(
        runner.close("lead").is_empty(),
        "the scripted call names no thread, so the record refused it"
    );
    assert_eq!(runner.tools().drain_refusals("lead").len(), 1);
}

/// A hosted runner over a test host whose log already holds the task.
fn hosted(library: LibraryHost, contract: &str) -> (Arc<TestHost>, HostedRunner<TestHost>) {
    assert!(format!("{library:?}").contains(offline::MODEL));
    let log = MemoryLog::new("engineering");
    log.append("operator", "state the root cause", None, None);
    let host = Arc::new(TestHost {
        log,
        library,
        prompt: format!("You lead the desk.\n\n{contract}"),
        wrapped: AtomicUsize::new(0),
        after: AtomicUsize::new(0),
        metered: AtomicBool::new(false),
        halt: AtomicBool::new(false),
    });
    let runner = HostedRunner::seat(
        Arc::clone(&host),
        Arc::new(EpisodeTools::new(["lead"])),
        &["lead".to_owned()],
        "engineering",
        "Engineering",
        SESSION_WINDOW,
    )
    .expect("hosted seats");
    assert!(format!("{runner:?}").contains("lead"));
    assert!(format!("{:?}", runner.bindings()[0].agent).contains("lead"));
    (host, runner)
}

/// The scripted model's one call, recorded once, as any runner records it.
fn one_completion(name: &str, events: &[SeatEvent]) {
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

/// A second turn on each native runner: raw is seeded with what it said,
/// hosted clears and reseeds the session it reuses, and both call again.
async fn again(raw: &RawRunner, host: &TestHost, hosted: &HostedRunner<TestHost>) {
    // A second raw turn is seeded with the first: what the seat said is what
    // it is shown, and the record starts empty again.
    let (_, again) = one_turn(raw, Sequence(0)).await;
    assert_eq!(again.len(), 1);
    host.log.append("lead", "COMPLETE: done", None, None);
    let (_, again) = one_turn(hosted, host.log.latest()).await;
    assert_eq!(again.len(), 1, "the reused session ran and called again");
    assert_eq!(host.wrapped.load(Ordering::SeqCst), 2);
}

/// A seat none of the runners seated is a failed turn, not a panic; and a
/// seat registered after the process registry is set is refused by name
/// rather than seated as a ghost.
async fn ghosts(embed: &EmbedRunner, raw: &RawRunner, hosted: &HostedRunner<TestHost>) {
    for outcome in [
        raw.turn("ghost".into(), Lane::Desk, Sequence(0), "?".into())
            .await,
        hosted
            .turn("ghost".into(), Lane::Desk, Sequence(0), "?".into())
            .await,
        embed
            .turn("ghost".into(), Lane::Desk, Sequence(0), "?".into())
            .await,
    ] {
        assert!(
            matches!(&outcome, (seat, Lane::Desk, Some(Err(why))) if seat == "ghost" && why.contains("not a seat")),
            "{outcome:?}"
        );
    }
    let elsewhere = tempfile::tempdir().expect("a workspace");
    let ghost = register_seats(elsewhere.path(), &[("ghost", "Nobody.")], &[]);
    assert!(
        matches!(&ghost, Err(crate::Error::SeatNotRegistered { seat }) if seat == "ghost"),
        "{ghost:?}"
    );
}

/// The hook halting is the turn failing: the host loop sees an error where
/// a reply would be, after the turn ran and its call landed.
async fn halts(host: &TestHost, hosted: &HostedRunner<TestHost>) {
    host.halt.store(true, Ordering::SeqCst);
    hosted.open(
        "lead",
        Vec::new(),
        Dispatch {
            chat: "engineering".into(),
            parent: None,
        },
    );
    let (_, _, halted) = hosted
        .turn(
            "lead".into(),
            Lane::Desk,
            host.log.latest(),
            "Once more.".into(),
        )
        .await;
    assert!(
        matches!(&halted, Some(Err(error)) if error.contains("budget is spent")),
        "{halted:?}"
    );
    assert_eq!(
        hosted.close("lead").len(),
        1,
        "the call it made before the halt stands"
    );
    // A turn in a thread the log does not have is seeded with nothing and
    // runs; the hook still runs after it, and halts it. Whichever side
    // fails, the hook has run once per started turn.
    let before = host.after.load(Ordering::SeqCst);
    host.halt.store(true, Ordering::SeqCst);
    hosted.open(
        "lead",
        Vec::new(),
        Dispatch {
            chat: "engineering".into(),
            parent: Some(u64::MAX.to_string()),
        },
    );
    let (_, _, failed) = hosted
        .turn(
            "lead".into(),
            Lane::Thread(Sequence(u64::MAX)),
            Sequence(u64::MAX),
            "Once more.".into(),
        )
        .await;
    assert_eq!(
        host.after.load(Ordering::SeqCst),
        before + 1,
        "the hook ran"
    );
    assert!(matches!(&failed, Some(Err(_))), "{failed:?}");
    hosted.close("lead");
    host.halt.store(false, Ordering::SeqCst);
}

/// The embed runtime, booted once per process over the scripted route.
async fn runtime(
    config: &openhuman_embed::RuntimeConfig,
    backend: &wiremock::MockServer,
    route: &Route,
    workspace: &std::path::Path,
) -> Runtime {
    Box::pin(
        Runtime::builder()
            .config(config.clone())
            .workspace(Workspace::dir(workspace.to_path_buf()))
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
    .expect("the runtime boots")
}

/// Every runner, offline, against one scripted model: the same call lands in
/// the record the same way, whichever road it took. One test rather than
/// two because the runtime and the definition registry are process-wide, and
/// the raw seats must be registered before the runtime boots.
///
/// On its own thread with a wide stack: an `OpenHuman` turn is a deep
/// composition of `async fn`s, and the two megabytes libtest gives a test
/// thread overflow on Linux before the first reply lands.
#[test]
fn both_runners_land_the_same_scripted_call_in_the_record() {
    std::thread::Builder::new()
        .stack_size(WIDE_STACK)
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(WIDE_STACK)
                .build()
                .expect("a runtime")
                .block_on(both_runners());
        })
        .expect("a thread")
        .join()
        .expect("the proof ran");
}

/// Sixteen megabytes: what the example's host loop gives its workers.
const WIDE_STACK: usize = 16 * 1024 * 1024;

async fn both_runners() {
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

    // One definition for `lead` names every tool either native runner hands
    // it: the served belt for raw, and the host's prefixed one for hosted.
    let named: Vec<String> = served_specs()
        .flat_map(|spec| [spec.name.to_owned(), format!("desk_{}", spec.name)])
        .collect();
    register_seats(workspace.path(), &[("lead", "You lead the desk.")], &named)
        .expect("seats register");
    let runtime = runtime(&config, &backend, &route, workspace.path()).await;
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

    let library = LibraryHost::boot(&config, &backend.uri(), &route, workspace.path())
        .await
        .expect("the library boots");
    let (host, hosted) = hosted(library, &contract(RunnerKind::Hosted));

    let (embed_reply, embed_events) = one_turn(&embed, Sequence(0)).await;
    let (raw_reply, raw_events) = one_turn(&raw, Sequence(0)).await;
    // Seeded from the host's log: the operator's row is history, not brief.
    let (hosted_reply, hosted_events) = one_turn(&hosted, host.log.latest()).await;
    assert_eq!(
        host.wrapped.load(Ordering::SeqCst),
        1,
        "the host wrapped the turn"
    );
    assert!(hosted.usage("lead").is_some(), "the turn's usage is kept");
    assert_eq!(host.after.load(Ordering::SeqCst), 1, "the hook ran once");
    assert!(host.metered.load(Ordering::SeqCst), "and saw the usage");
    assert_eq!(hosted_reply, raw_reply);
    for (name, events) in [
        ("embed", &embed_events),
        ("raw", &raw_events),
        ("hosted", &hosted_events),
    ] {
        one_completion(name, events);
    }
    assert_eq!(
        embed_reply, raw_reply,
        "the closing sentence is the model's"
    );
    let seen = metrics.snapshot();
    assert_eq!(seen.round_trips.len(), 3, "one receipted call per runner");
    assert!(seen.requests >= 6, "each turn is a call and a receipt");

    again(&raw, &host, &hosted).await;
    ghosts(&embed, &raw, &hosted).await;
    plain(host.library.clone(), &contract(RunnerKind::Hosted)).await;
    halts(&host, &hosted).await;
    metrics.reset();
    assert_eq!(metrics.snapshot().requests, 0);
}
