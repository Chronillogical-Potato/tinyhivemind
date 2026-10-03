//! One completion-driven episode, conducted over real OpenHuman agents.
//!
//! The point of this binary is not the task. It is the shape of the host:
//! the room's tools are `tinyhivemind-tools`' record, handed to seats natively;
//! the loop is
//! `tinyhivemind-openhuman`'s driver stepped the way a host steps it --
//! propose a round, run it, commit what it said, report delivery, repeat until
//! quiescent -- and what is written here is only what a host owns: agents,
//! a journal, sessions, and the prompt a turn is shown.
//!
//! How a seat's turn *runs* is behind one seam, `tinyhivemind_openhuman::SeatRunner`,
//! with two implementations: `openhuman-embed` agents with native tools
//! (`EmbedRunner`, the default), and the host's own seats seeded from its
//! journal (`HostedRunner`, over `conducted::hosted`). The loop cannot tell
//! them apart; `TINYHIVEMIND_RUNNER=hosted` selects the latter.
//!
//! ```sh
//! set -a; . ~/.config/tinyhivemind/live.env; set +a
//! TINYHIVEMIND_LIVE_OPENROUTER=1 cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! # Offline, either runner runs against a scripted model as a proof of its mechanics:
//! TINYHIVEMIND_RUNNER=hosted cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! # Compare both runners, N episodes each, as one table of what the harness costs:
//! CONDUCTED_BENCH=5 cargo run --release --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! ```

mod conducted {
    pub mod hosted;
    pub mod jev;
    pub mod routing;
    pub mod scenarios;
}

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use conducted::hosted::{DESK_PREAMBLE, DeskHost, DeskJournal};
use conducted::jev::LiveJev;
use conducted::routing::{candidate, policy};
use openhuman_embed::{Access, Provider, Runtime, RuntimeConfig, ServiceSet, Workspace};
use tinyhivemind_core::runtime::SESSION_WINDOW;
use tinyhivemind_core::runtime::desk::{Desk, ResponderMode};
use tinyhivemind_core::driver::{
    BoundHive, BroadcastRouting, CompletionDriver, ConductPolicy, Door, HiveGraph,
    standing_contract,
};
use tinyhivemind_core::embed::{
    ConversationKind, ConversationRef, RouteCandidate, Router, RouterFuture, RoutingPlan, RoutingRequest,
    RoutingSource, route_message,
};
use tinyhivemind_openhuman::{
    EmbedRunner, HostedRunner, Journal, LibraryHost, MemoryLog, Route, RunnerKind, SeatRunner,
    offline, register_seats, run_episode,
};
use tinyhivemind_tools::{EpisodeTools, served_specs};
use tinyhivemind_core::typesafe::JevRouter;



use conducted::scenarios::{Scenario, scenario_from_env};

const JEV_MODEL: &str = "jev-1.13.0";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(WORKER_STACK_BYTES)
        .build()?
        .block_on(run())
}

/// A router that counts its calls: the provider bill, one line.
struct Counted<R> {
    inner: R,
    calls: AtomicU64,
}

impl<R: Router> Router for Counted<R> {
    fn evaluate<'a>(&'a self, request: &'a RoutingRequest) -> RouterFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.evaluate(request)
    }
}

async fn run() -> anyhow::Result<()> {
    let scenario = scenario_from_env()?;
    let desk_id = scenario.id;
    let _ = env_logger::builder().is_test(false).try_init();
    let kind = RunnerKind::from_env()?;
    let live = std::env::var_os("TINYHIVEMIND_LIVE_OPENROUTER").is_some();
    let bench = match std::env::var("CONDUCTED_BENCH") {
        Ok(value) => Some(value.parse::<u32>().map_err(|_| {
            anyhow::anyhow!("CONDUCTED_BENCH={value}: episodes per runner, a whole number")
        })?),
        Err(_) => None,
    };
    if live && bench.is_some() {
        anyhow::bail!("CONDUCTED_BENCH runs offline; unset TINYHIVEMIND_LIVE_OPENROUTER");
    }
    if !live {
        eprintln!(
            "conducted: offline, against a scripted model, as a proof of mechanics. Set\n\
             TINYHIVEMIND_LIVE_OPENROUTER=1, OPENROUTER_API_KEY, OPENROUTER_MODEL and\n\
             TYPESAFE_API_KEY for a live episode: one model call per turn, one Jev call per route."
        );
    }

    // The core makes non-inference backend calls; signed out of the real one
    // those hang rather than fail. Stub them.
    let backend = offline::backend().await;

    let run_id = std::process::id().to_string();
    let run_dir = std::env::temp_dir().join(format!("tinyhivemind-conducted-{run_id}"));
    let workspace = run_dir.join("openhuman-runtime");
    std::fs::create_dir_all(&workspace)?;

    // Where inference comes from: OpenRouter, or -- with no credential -- the
    // scripted model, which also keeps the harness metrics the bench prints.
    let metrics = Arc::new(offline::Metrics::default());
    let scripted = if live {
        None
    } else {
        Some(offline::model(desk_id, Arc::clone(&metrics)).await)
    };
    let route = match &scripted {
        None => Route {
            endpoint: OPENROUTER.to_owned(),
            api_key: required("OPENROUTER_API_KEY")?,
            model: required("OPENROUTER_MODEL")?,
        },
        Some(server) => Route {
            endpoint: format!("{}/v1", server.uri()),
            api_key: "local-test-key".to_owned(),
            model: offline::MODEL.to_owned(),
        },
    };

    let ids: Vec<String> = scenario
        .seats
        .iter()
        .map(|(id, _, _)| (*id).to_owned())
        .collect();
    let mut config = if live {
        RuntimeConfig::load_or_init().await?
    } else {
        offline::config()
    };
    config.agent.max_tool_iterations = 6;
    config.default_temperature = 0.0;
    let briefs: BTreeMap<String, String> = scenario
        .seats
        .iter()
        .map(|(id, role, known)| {
            (
                (*id).to_owned(),
                format!("{role}\n\n## What you privately know\n{known}"),
            )
        })
        .collect();
    let candidates: Vec<RouteCandidate> = scenario
        .seats
        .iter()
        .map(|(id, role, _)| candidate(id, role))
        .collect();
    let host = Host {
        scenario,
        run_id,
        workspace,
        backend_url: backend.uri(),
        route,
        config,
        ids,
        briefs,
        candidates,
        live,
    };

    let result = match bench {
        Some(episodes) => bench_runners(&host, kind, episodes, &metrics).await,
        None => {
            println!("[runner] {}", kind.name());
            let journal = Arc::new(MemoryLog::new(desk_id));
            let desk = Arc::new(host.journal(&journal, false));
            let report = match kind {
                RunnerKind::Embed => {
                    let runtime = host.runtime().await?;
                    let runner = host.embed(Arc::clone(&desk), &runtime, 0).await?;
                    episode(runner, host.setup(kind, false), journal, &*desk).await?
                }
                RunnerKind::Hosted => {
                    host.prepare_seats()?;
                    let (runner, desk) = host.hosted(&journal, false).await?;
                    episode(runner, host.setup(kind, false), journal, &*desk).await?
                }
            };
            let _ = report;
            Ok(())
        }
    };
    drop(scripted);
    result
}

/// Everything a run holds that both runners and every episode share.
struct Host {
    scenario: &'static Scenario,
    run_id: String,
    workspace: std::path::PathBuf,
    backend_url: String,
    route: Route,
    config: RuntimeConfig,
    ids: Vec<String>,
    briefs: BTreeMap<String, String>,
    candidates: Vec<RouteCandidate>,
    live: bool,
}

impl Host {
    /// The standing contract for `kind`: the vocabulary's words for exactly
    /// the served tools, plus the runner's one sentence on the mechanics.
    fn contract(&self, kind: RunnerKind) -> String {
        // From the record's own `specs()`, not the crate-wide list: a host
        // that withholds a tool must not describe it in the contract it hands
        // its seats. This desk withholds nothing, and says so by asking the
        // same object the seats are served from.
        let tools = self.tools();
        format!(
            "{DESK_PREAMBLE}\n\n{}",
            standing_contract(
                tools.specs(),
                self.scenario.id,
                &self.ids,
                kind.how_to_call()
            )
        )
    }

    /// The record this desk's seats call into: every seat, every tool.
    fn tools(&self) -> Arc<EpisodeTools> {
        Arc::new(EpisodeTools::new(self.ids.iter().cloned()))
    }

    fn setup(&self, kind: RunnerKind, quiet: bool) -> Setup {
        Setup {
            scenario: self.scenario,
            kind,
            ids: self.ids.clone(),
            candidates: self.candidates.clone(),
            live: self.live,
            quiet,
        }
    }

    /// The `openhuman-embed` runtime the embed runner seats on. One per
    /// process: the runtime refuses a second, so a bench builds it once.
    async fn runtime(&self) -> anyhow::Result<Runtime> {
        Ok(Runtime::builder()
            .config(self.config.clone())
            .workspace(Workspace::dir(self.workspace.clone()))
            .services(ServiceSet::none())
            .backend_url(self.backend_url.clone())
            .provider(
                Provider::openai_compatible(
                    self.route.endpoint.clone(),
                    self.route.api_key.clone(),
                )
                .model(self.route.model.clone()),
            )
            // The native tool belt needs write-capable access. Its allowlist
            // contains the episode tools and no shell.
            .access(Access::full())
            .build()
            .await?)
    }

    /// Seat the embed runner for one episode. `episode` keeps agent ids
    /// unique across a bench's episodes on the one runtime.
    async fn embed(
        &self,
        desk: Arc<DeskJournal>,
        runtime: &Runtime,
        episode: u32,
    ) -> anyhow::Result<EmbedRunner> {
        let (journal, tools, briefs, contract) = (
            desk,
            self.tools(),
            &self.briefs,
            self.contract(RunnerKind::Embed),
        );
        let (desk_id, desk_name) = (self.scenario.id, self.scenario.name);
        let run_id = format!("{}-{episode}", self.run_id);
        let seated = EmbedRunner::seat(
            journal,
            runtime,
            tools,
            briefs,
            &contract,
            desk_id,
            desk_name,
            tinyhivemind_core::runtime::SESSION_WINDOW,
            &run_id,
        );
        seated.map_err(Into::into)
    }

    /// A hosted seat is resolved against the process registry, so register
    /// every seat before the hosted runner starts.
    fn prepare_seats(&self) -> anyhow::Result<()> {
        let seats: Vec<(&str, &str)> = self
            .scenario
            .seats
            .iter()
            .map(|(id, role, _)| (*id, *role))
            .collect();
        let belt: Vec<String> = served_specs().map(|spec| spec.name.to_owned()).collect();
        Ok(register_seats(&self.workspace, &seats, &belt)?)
    }

    /// This desk as a journal over `journal`, for every runner.
    fn journal(&self, journal: &Arc<MemoryLog>, quiet: bool) -> DeskJournal {
        DeskJournal::new(Arc::clone(journal), self.briefs.clone(), quiet)
    }

    /// Seat the hosted runner over `journal`, which is also the log the
    /// episode appends to, and return the host it was seated on, which is
    /// the episode's journal too. Its seats are registered by
    /// `prepare_seats`, the same definitions the hosted seats resolve.
    async fn hosted(
        &self,
        journal: &Arc<MemoryLog>,
        quiet: bool,
    ) -> anyhow::Result<(HostedRunner<DeskHost>, Arc<DeskHost>)> {
        let library = LibraryHost::boot(
            &self.config,
            &self.backend_url,
            &self.route,
            &self.workspace,
        )
        .await?;
        let contract = self.contract(RunnerKind::Hosted);
        let prompts = self
            .briefs
            .iter()
            .map(|(id, brief)| (id.clone(), format!("{brief}\n\n{contract}")))
            .collect();
        // The hosted runner seats `AgentSpec` agents, so it needs a runtime.
        // Safe to boot one here: `RunnerKind::from_env` picks a single runner
        // per process, so the embed path is not also holding the slot.
        let runtime = Arc::new(self.runtime().await?);
        let host = Arc::new(DeskHost::new(
            self.journal(journal, quiet),
            library,
            runtime,
            prompts,
        ));
        let runner = HostedRunner::seat(
            Arc::clone(&host),
            self.tools(),
            &self.ids,
            self.scenario.id,
            self.scenario.name,
            SESSION_WINDOW,
        )?;
        Ok((runner, host))
    }
}

/// Every runner, `episodes` times each, offline, and one table.
///
/// The model is scripted, so nothing here is about answers: every seat
/// completes on its first turn. What differs between the arms is the host --
/// the road a tool call takes, what a turn costs to set up, and how much is
/// sent to the model -- and that is what the columns are.
///
/// `first` runs first. The arms share one process, so each begins with an
/// episode that is run and not counted; `TINYHIVEMIND_RUNNER` names the arm
/// that goes first, and a difference that survives every order is the
/// harness's.
async fn bench_runners(
    host: &Host,
    first: RunnerKind,
    episodes: u32,
    metrics: &offline::Metrics,
) -> anyhow::Result<()> {
    struct Arm {
        kind: RunnerKind,
        reports: Vec<Report>,
        seen: offline::Snapshot,
    }
    let mut arms: Vec<Arm> = Vec::new();
    // The runtime reads the process registry as it boots, so register seats
    // before creating either runner.
    host.prepare_seats()?;
    let runtime = host.runtime().await?;
    let order: Vec<RunnerKind> = std::iter::once(first)
        .chain(
            [RunnerKind::Embed, RunnerKind::Hosted]
                .into_iter()
                .filter(|kind| *kind != first),
        )
        .collect();
    for kind in order {
        println!("[bench] {} x{episodes}", kind.name());
        let runtime = &runtime;
        let run = move |index: u32| async move {
            let journal = Arc::new(MemoryLog::new(host.scenario.id));
            let desk = Arc::new(host.journal(&journal, true));
            match kind {
                RunnerKind::Embed => {
                    let runner = host.embed(Arc::clone(&desk), runtime, index).await?;
                    episode(runner, host.setup(kind, true), journal, &*desk).await
                }
                RunnerKind::Hosted => {
                    let (runner, desk) = host.hosted(&journal, true).await?;
                    episode(runner, host.setup(kind, true), journal, &*desk).await
                }
            }
        };
        // One episode nobody counts: the first turn through either harness
        // pays for page faults, lazy statics and a cold allocator, and which
        // arm paid it used to be whichever went first.
        run(0).await?;
        metrics.reset();
        let mut reports = Vec::new();
        for index in 1..=episodes {
            reports.push(run(index).await?);
        }
        arms.push(Arm {
            kind,
            reports,
            seen: metrics.snapshot(),
        });
    }
    println!();
    println!(
        "{:<7} {:>8} {:>9} {:>9} {:>13} {:>10} {:>14} {:>10}",
        "runner",
        "episodes",
        "turns/ep",
        "waves/ep",
        "requests/turn",
        "KiB/turn",
        "tool rtt ms",
        "wall ms/ep"
    );
    for arm in &arms {
        let turns: u64 = arm.reports.iter().map(|r| r.turns).sum();
        let waves: u64 = arm.reports.iter().map(|r| r.waves).sum();
        let n = arm.reports.len() as f64;
        let per_turn = |value: f64| {
            if turns == 0 {
                0.0
            } else {
                value / turns as f64
            }
        };
        let wall: f64 = arm
            .reports
            .iter()
            .map(|r| r.wall.as_secs_f64() * 1000.0)
            .sum::<f64>()
            / n;
        let rtt = if arm.seen.round_trips.is_empty() {
            0.0
        } else {
            arm.seen
                .round_trips
                .iter()
                .map(|d| d.as_secs_f64() * 1000.0)
                .sum::<f64>()
                / arm.seen.round_trips.len() as f64
        };
        println!(
            "{:<7} {:>8} {:>9.1} {:>9.1} {:>13.2} {:>10.1} {:>14.1} {:>10.0}",
            arm.kind.name(),
            arm.reports.len(),
            turns as f64 / n,
            waves as f64 / n,
            per_turn(arm.seen.requests as f64),
            per_turn(arm.seen.bytes as f64 / 1024.0),
            rtt,
            wall
        );
    }
    println!(
        "\nturns/ep: seat turns the loop ran; waves/ep: rounds it took. requests/turn: model calls per turn. \
         KiB/turn: request bytes to the model. tool rtt: from the model emitting a \
         tool call to its receipt, the whole harness in between. wall: one episode, end to end."
    );
    Ok(())
}

/// What the episode needs from setup, whichever runner runs it.
#[derive(Clone)]
struct Setup {
    scenario: &'static Scenario,
    kind: RunnerKind,
    ids: Vec<String>,
    candidates: Vec<RouteCandidate>,
    live: bool,
    /// Skip the journal dump at the end: a bench prints one table instead.
    quiet: bool,
}

/// What one episode came to, for a bench to add up.
#[derive(Clone, Copy, Debug)]
struct Report {
    turns: u64,
    waves: u64,
    wall: std::time::Duration,
}

/// One episode over `runner`, stepped to quiescence.
///
/// Generic rather than boxed so each runner's bound seat type flows into the
/// hive and the driver as itself: the loop never names it. What is here is
/// only what a host owns: the journal, the prompt, running a turn, and the
/// log. The rules -- conversations, nudges, what a wave said and where it
/// goes, refusals, walls -- are the conductor's.
async fn episode<R: SeatRunner, J: Journal>(
    runner: R,
    setup: Setup,
    journal: Arc<MemoryLog>,
    desk: &J,
) -> anyhow::Result<Report> {
    let Setup {
        scenario,
        kind,
        ids,
        candidates,
        live,
        quiet,
    } = setup;
    let started = std::time::Instant::now();
    let desk_id = scenario.id;
    let bindings = runner.bindings();
    let seated: BTreeSet<&str> = bindings.iter().map(|b| b.hive_agent_id.as_str()).collect();
    let advertised: BTreeSet<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
    anyhow::ensure!(
        seated == advertised,
        "roster drift: seated {seated:?} but advertising {advertised:?}"
    );
    let hive = BoundHive::new(
        HiveGraph::new(
            Desk {
                id: desk_id.into(),
                name: scenario.name.into(),
                description: Some(scenario.description.into()),
                members: ids.clone(),
                responder_mode: ResponderMode::Auto,
            },
            candidates.clone(),
        ),
        bindings,
    )?;

    // Jev routes for real only live; offline every route is the deterministic
    // fallback, and the loop does not change either way. The fallback seat is
    // the desk's first seat: `theory` on the login desk, `dispatcher` on triage.
    let fallback = scenario.seats[0].0;
    let router = if live {
        Some(Counted {
            inner: JevRouter::with_model(LiveJev::from_env()?, JEV_MODEL),
            calls: AtomicU64::new(0),
        })
    } else {
        None
    };
    let primary: Option<&(dyn Router + '_)> = router.as_ref().map(|r| r as &(dyn Router + '_));
    let opened_at = journal.append("operator", scenario.task, None, &[]);

    // The door route: who starts.
    let door = RoutingRequest {
        message: scenario.task.to_owned(),
        source: RoutingSource::DeskMessage,
        conversation: ConversationRef {
            id: desk_id.into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: Some(scenario.description.to_owned()),
        thread_context: Vec::new(),
        candidates: candidates.clone(),
        roster_version: 1,
        policy: policy(scenario.door_width),
    };
    let plan = route_message(primary, None, &door, None, fallback).await;
    if matches!(plan, RoutingPlan::Clarify { .. }) {
        eprintln!("[door] routing asked for clarification; {fallback} owns it");
    }
    let starters = tinyhivemind_core::driver::starters(&plan, fallback);
    println!("[door] starts: {}", starters.join(", "));

    // Round width four: the door can start up to four seats and they run
    // together. The broadcast policy is width one -- a handoff belongs to one
    // seat -- and the driver clamps routing to the smaller of the two.
    let driver = CompletionDriver::new(&hive, 4)?
        .with_queue_depth(2)?
        .with_broadcast_budget(Some(2));
    let broadcast_policy = policy(1);
    let routing = BroadcastRouting {
        primary,
        reasoning: None,
        policy: &broadcast_policy,
        roster_version: 1,
        thread_context: &[],
    };
    let outcome = run_episode(
        desk,
        &runner,
        &driver,
        routing,
        ConductPolicy::default(),
        Door {
            chat: desk_id.into(),
            desk_name: scenario.name.into(),
            members: ids.clone(),
            starters,
            opened_at,
        },
    )
    .await;
    let report = match &outcome {
        Ok(report) => *report,
        Err(_) => tinyhivemind_openhuman::Report::default(),
    };
    println!(
        "turns {} | routes {} | waves {} | discharged {} | settled {} | conversations {}",
        report.turns,
        router
            .as_ref()
            .map_or(0, |r| r.calls.load(Ordering::SeqCst)),
        report.waves,
        report.discharged,
        report.settled,
        report.conversations
    );
    for row in journal.all().iter().filter(|_| !quiet) {
        let scope = match (row.thread, row.only_for.as_slice()) {
            (Some(root), _) => format!(" (thread {})", root.0),
            (None, []) => String::new(),
            (None, only) => format!(" (to @{})", only.join(", @")),
        };
        println!(
            "  {:>3}  @{}{scope}: {}",
            row.sequence.0, row.author, row.body
        );
    }
    let settled: anyhow::Result<()> = outcome.map(|_| ()).map_err(Into::into);
    settled?;
    // Offline, the run is a proof: the scripted seat's native tool call must
    // have become a desk row through the shared record.
    if !live {
        let expected = format!("COMPLETE: {}", offline::COMPLETION);
        anyhow::ensure!(
            journal.all().iter().any(|row| row.body == expected),
            "the scripted completion never reached the journal"
        );
        if !quiet {
            println!(
                "offline proof: a {} tool call became a desk row",
                match kind {
                    RunnerKind::Embed => "spec-belt native",
                    RunnerKind::Hosted => "hosted native",
                }
            );
        }
    }
    let report = Report {
        turns: report.turns,
        waves: report.waves,
        wall: started.elapsed(),
    };
    drop(runner);
    Ok(report)
}

/// Nothing running but the one subsystem the episode's tools arrive through.
fn required(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|_| anyhow::anyhow!("{name} must be set for a live run"))
}
