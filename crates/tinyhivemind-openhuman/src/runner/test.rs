//! The seam: which runner the environment names, and both runners through it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use openhuman_core::agent::tinyagents::host::LastTurnUsage;
use openhuman_embed::{Access, Provider, Runtime, Workspace};
use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind::{SESSION_WINDOW, Sequence, SessionLog};
use tinyhivemind_driver::standing_contract;
use tinyhivemind_tools::{Dispatch, EpisodeTools, SeatEvent, served_specs};

use super::{Lane, RunnerKind, SeatRunner};
use crate::MemoryLog;
use crate::{
    Disposition, EmbedRunner, EpisodeBeltSource, EpisodeHost, HostedRunner, HostedTurn, Journal,
    LibraryHost, RawRunner, Route, TurnResult, offline, register_seats,
};
use openhuman_embed::Agent;
use tinyhivemind_driver::{Commit, Note};

/// A host with no agents of its own: its seats are library sessions, its
/// log is in memory, and its wrapper is the core context a library session
/// needs -- which is exactly what a real host installs there.
struct TestHost {
    log: MemoryLog,
    library: LibraryHost,
    /// Seats are `AgentSpec` agents now, so the host holds the runtime it
    /// registers them on -- the same one the other runners in this test use,
    /// because a process has exactly one.
    runtime: Arc<Runtime>,
    prompt: String,
    wrapped: AtomicUsize,
    /// Turns the hook saw, and whether any came with usage.
    after: AtomicUsize,
    metered: AtomicBool,
    /// Whether the hook halts the episode on the next turn.
    halt: AtomicBool,
    /// The next turn stops on the host instead of standing.
    park: AtomicBool,
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

    fn display_name(&self, seat: &str) -> String {
        match seat {
            "lead" => "Lena".to_owned(),
            other => other.to_owned(),
        }
    }
}

impl EpisodeHost for TestHost {
    fn build_seat(&self, seat: &str, belt: EpisodeBeltSource) -> crate::Result<Agent> {
        seat_agent(&self.runtime, seat, &self.prompt, belt)
    }

    /// Namespaced per host, not just per seat. Both hosts in this file seat a
    /// `lead` on the one runtime a process has, so a bare
    /// `episode:engineering:lead` would name the *same* session for two
    /// scenarios with separate logs. Every turn seeds, which clears whatever
    /// the session held -- but relying on that to keep two tests apart makes
    /// them order-dependent for no gain.
    fn seat_session(&self, seat: &str) -> String {
        format!("episode:engineering:test:{seat}")
    }

    fn wrap_turn<'a>(&'a self, _seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        self.wrapped.fetch_add(1, Ordering::SeqCst);
        Box::pin(self.library.scope(turn))
    }

    fn tool_prefix(&self) -> String {
        "desk_".into()
    }

    fn after_turn(&self, seat: &str, usage: Option<&LastTurnUsage>) -> crate::Result<Disposition> {
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
        if self.park.swap(false, Ordering::SeqCst) {
            return Ok(Disposition::Parked);
        }
        Ok(Disposition::Done)
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
async fn one_turn<R: SeatRunner>(runner: &R, since: Option<Sequence>) -> (String, Vec<SeatEvent>) {
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
    let reply = reply.reply().map(str::to_owned).expect("the turn ran");
    (reply, runner.close("lead"))
}

/// A host that overrides nothing it need not: no prefix, no wrapper, no
/// hook. Its seat runs under the process default context the library boot
/// installed, which is what a host with a booted core of its own has.
struct PlainHost {
    log: MemoryLog,
    runtime: Arc<Runtime>,
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
            commit.only_for.as_deref(),
        ))
    }

    fn note(&self, note: &Note) -> crate::Result<()> {
        self.log
            .append("desk", &note.body, note.thread, note.only_for.as_deref());
        Ok(())
    }
}

impl EpisodeHost for PlainHost {
    fn build_seat(&self, seat: &str, belt: EpisodeBeltSource) -> crate::Result<Agent> {
        seat_agent(&self.runtime, seat, "You lead the desk.", belt)
    }

    /// Namespaced per host, not just per seat. Both hosts in this file seat a
    /// `lead` on the one runtime a process has, so a bare
    /// `episode:engineering:lead` would name the *same* session for two
    /// scenarios with separate logs. Every turn seeds, which clears whatever
    /// the session held -- but relying on that to keep two tests apart makes
    /// them order-dependent for no gain.
    fn seat_session(&self, seat: &str) -> String {
        format!("episode:engineering:plain:{seat}")
    }
}

/// A number no other seating in this process has used.
fn seating() -> usize {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    NEXT.fetch_add(1, Ordering::SeqCst)
}

/// One seat, as an `AgentSpec` whose belt is the episode's, rebuilt per turn.
///
/// This is the shape a real host uses: the belt is not handed over once and
/// held, it is made again for every turn out of the source, because that is
/// what `AgentSpec::tools` asks for and what lets one agent serve an episode
/// and its ordinary work without existing twice.
fn seat_agent(
    runtime: &Runtime,
    seat: &str,
    prompt: &str,
    belt: EpisodeBeltSource,
) -> crate::Result<Agent> {
    Ok(runtime.agent(
        // Not the bare seat id. A runtime id is unique per process, and this
        // one is taken twice over: `register_seats` registered `lead` for the
        // raw runner, and both hosted hosts in this file seat a `lead` of
        // their own. The hive still knows the seat as `lead` -- that is the
        // binding's id, not the runtime's, and only the latter has to be
        // unique here.
        openhuman_embed::AgentSpec::new(format!("{seat}-hosted-{}", seating()))
            .system_prompt(prompt.to_owned())
            .tools(move |_turn| {
                let belt = belt.belt();
                let policy = belt.admit(None);
                openhuman_embed::HostTurnTools::advertised(belt.tools).with_policy(policy)
            }),
    )?)
}

/// A hosted seat on a host that keeps every default, run once on the desk
/// and once in a thread it is not in: the defaults hold, the thread turn is
/// seeded from the thread, and a call outside its thread is refused.
async fn plain(runtime: &Arc<Runtime>) {
    let log = MemoryLog::new("engineering");
    log.append("operator", "state the root cause", None, None);
    let host = Arc::new(PlainHost {
        log,
        runtime: Arc::clone(runtime),
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
    assert_eq!(
        runner.tools().display_name("lead"),
        "lead",
        "a host that names nobody leaves the record's seats by id"
    );
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
            Some(Sequence(1)),
            "In the thread.".into(),
        )
        .await;
    assert_eq!(lane, Lane::Thread(Sequence(1)));
    assert!(matches!(outcome, TurnResult::Replied(_)), "{outcome:?}");
    assert!(
        runner.close("lead").is_empty(),
        "the scripted call names no thread, so the record refused it"
    );
    assert_eq!(runner.tools().drain_refusals("lead").len(), 1);
}

/// A failed turn reports what the session counted, and leaves no stale
/// number behind under its seat.
///
/// The fault lands on the second request -- the one carrying the tool's
/// receipt -- so the turn has already called a tool and been answered once
/// by the time it dies. Metering rides a callback rather than the outcome
/// so that this turn is read before its error is raised; the callback is
/// only worth having if nothing between it and the hook drops what it
/// caught, and before this the two writes sat *below* the `?` that carries
/// the error out.
///
/// What the callback carries here is `None`, and that is upstream's answer
/// rather than a dropped value: `last_turn_usage` is recorded after the
/// turn's durable commit, so a turn that dies before one never counted. The
/// assertion that matters is therefore the other half -- the seat's entry
/// is *cleared*, not left reading the previous turn's spend, which is what
/// an early return used to leave behind. Should a failing turn ever arrive
/// carrying usage, it now reaches the hook instead of the floor.
async fn spent(faults: &offline::Faults, host: &TestHost, hosted: &HostedRunner<TestHost>) {
    host.halt.store(false, Ordering::SeqCst);
    host.park.store(false, Ordering::SeqCst);
    // The hook flips this back on when it is handed usage, so clearing it
    // first makes the assertion about *this* turn rather than an earlier one.
    host.metered.store(false, Ordering::SeqCst);
    let before = host.after.load(Ordering::SeqCst);
    faults.fail_the_receipt(true);
    hosted.open(
        "lead",
        Vec::new(),
        Dispatch {
            chat: "engineering".into(),
            parent: None,
        },
    );
    let (_, _, failed) = hosted
        .turn(
            "lead".into(),
            Lane::Desk,
            host.log.latest(),
            "Once more.".into(),
        )
        .await;
    faults.fail_the_receipt(false);
    assert!(matches!(failed, TurnResult::Failed(_)), "{failed:?}");
    assert_eq!(
        host.after.load(Ordering::SeqCst),
        before + 1,
        "the hook ran for the failed turn"
    );
    assert!(
        !host.metered.load(Ordering::SeqCst),
        "with nothing to meter: the session counts a turn at its commit, and \
         this one did not reach one"
    );
    assert!(
        hosted.usage("lead").is_none(),
        "and the seat's entry is cleared rather than still reading the \
         previous turn's spend"
    );
    hosted.close("lead");
}

/// A hosted runner over a test host whose log already holds the task.
fn hosted(
    library: LibraryHost,
    runtime: &Arc<Runtime>,
    contract: &str,
) -> (Arc<TestHost>, HostedRunner<TestHost>) {
    assert!(format!("{library:?}").contains(offline::MODEL));
    let log = MemoryLog::new("engineering");
    log.append("operator", "state the root cause", None, None);
    let host = Arc::new(TestHost {
        log,
        library,
        runtime: Arc::clone(runtime),
        prompt: format!("You lead the desk.\n\n{contract}"),
        wrapped: AtomicUsize::new(0),
        after: AtomicUsize::new(0),
        metered: AtomicBool::new(false),
        halt: AtomicBool::new(false),
        park: AtomicBool::new(false),
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
    assert_eq!(
        runner.tools().display_name("lead"),
        "Lena",
        "the record is named by the host before any seat is built"
    );
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
    let (_, again) = one_turn(raw, None).await;
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
        raw.turn("ghost".into(), Lane::Desk, None, "?".into()).await,
        hosted
            .turn("ghost".into(), Lane::Desk, None, "?".into())
            .await,
        embed
            .turn("ghost".into(), Lane::Desk, None, "?".into())
            .await,
    ] {
        assert!(
            matches!(&outcome, (seat, Lane::Desk, TurnResult::Failed(why)) if seat == "ghost" && why.contains("not a seat")),
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
        matches!(&halted, TurnResult::Failed(error) if error.contains("budget is spent")),
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
            Some(Sequence(u64::MAX)),
            "Once more.".into(),
        )
        .await;
    assert_eq!(
        host.after.load(Ordering::SeqCst),
        before + 1,
        "the hook ran"
    );
    assert!(matches!(&failed, TurnResult::Failed(_)), "{failed:?}");
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
    let faults = Arc::new(offline::Faults::default());
    let model =
        offline::model_with_faults("engineering", Arc::clone(&metrics), Arc::clone(&faults)).await;
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
    let runtime = Arc::new(runtime(&config, &backend, &route, workspace.path()).await);
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
    let (host, hosted) = hosted(library, &runtime, &contract(RunnerKind::Hosted));

    let (embed_reply, embed_events) = one_turn(&embed, None).await;
    let (raw_reply, raw_events) = one_turn(&raw, None).await;
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
    plain(&runtime).await;
    halts(&host, &hosted).await;
    spent(&faults, &host, &hosted).await;
    metrics.reset();
    assert_eq!(metrics.snapshot().requests, 0);
}
