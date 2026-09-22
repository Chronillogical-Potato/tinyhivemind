//! One completion-driven episode, conducted over real OpenHuman agents.
//!
//! The point of this binary is not the task. It is the shape of the host:
//! the room's tools are `tinyhivemind-tools`' record, served over MCP by
//! `tinyhivemind-mcp` where a seat needs the wire, the loop is
//! `tinyhivemind-openhuman`'s driver stepped the way a host steps it --
//! propose a round, run it, commit what it said, report delivery, repeat until
//! quiescent -- and what is written here is only what a host owns: agents,
//! a journal, sessions, and the prompt a turn is shown.
//!
//! How a seat's turn *runs* is behind one seam, [`conducted::runner::SeatRunner`],
//! with two implementations: `openhuman-embed` agents reaching the tools over
//! MCP ([`conducted::embed`], the default), and raw `OpenHumanSessionHost`
//! sessions handed the same tools natively ([`conducted::raw`]). The loop
//! cannot tell them apart; `TINYHIVEMIND_RUNNER=raw` picks the second.
//!
//! ```sh
//! set -a; . ~/.config/tinyhivemind/live.env; set +a
//! TINYHIVEMIND_LIVE_OPENROUTER=1 cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! # Offline, either runner runs against a scripted model as a proof of its mechanics:
//! TINYHIVEMIND_RUNNER=raw cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! # And both, N episodes each, as one table of what the harness costs:
//! CONDUCTED_BENCH=5 cargo run --release --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! ```

mod conducted {
    pub mod embed;
    pub mod jev;
    pub mod raw;
    pub mod runner;
}

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use conducted::embed::EmbedRunner;
use conducted::jev::LiveJev;
use conducted::raw::offline;
use conducted::raw::{RawRunner, Route};
use conducted::runner::{Lane, RunnerKind, SeatRunner, TurnJob};
use openhuman_embed::{Access, Provider, Runtime, RuntimeConfig, ServiceSet, Workspace};
use serde_json::json;
use tinyhivemind::desk::{Desk, ResponderMode};
use tinyhivemind::responder::Probability;
use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind::{Conversation, Sequence};
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, RouteCandidate, Router, RouterFuture, RoutingPlan,
    RoutingPolicy, RoutingRequest, RoutingSource, route_message,
};
use tinyhivemind_hive::{CompletionEpisodeState, apply_completion};
use tinyhivemind_openhuman::{
    BoundAgent, BroadcastRouting, Channel, CommittedUtterance, CompletionDriver, ConversationView,
    DriverState, EpisodeBrief, Error, HiveGraph, HostAction, OpenHumanHive, standing_contract,
};
use tinyhivemind_tools::{Dispatch, EpisodeTools};
use tinyhivemind_typesafe::JevRouter;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// What the host says about the desk, before the episode's own contract.
const DESK_PREAMBLE: &str = "\
You are one seat on a desk. You have no codebase, shell or filesystem -- only
the desk's messages and your own judgement. Never ask for permission and never
wait to be told to continue; nobody will answer.";

/// A desk: who sits at it, what each seat privately knows, and the task.
struct Scenario {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    task: &'static str,
    /// `(seat id, role, what it alone knows)`: a hidden profile, so no seat
    /// can answer alone and the tools are necessary rather than available.
    seats: &'static [(&'static str, &'static str, &'static str)],
}

/// Which desk runs: `CONDUCTED_DESK=login` (default) or `triage`.
fn scenario_from_env() -> anyhow::Result<&'static Scenario> {
    match std::env::var("CONDUCTED_DESK").as_deref() {
        Err(_) | Ok("login") => Ok(&LOGIN),
        Ok("triage") => Ok(&TRIAGE),
        Ok(other) => Err(anyhow::anyhow!(
            "CONDUCTED_DESK={other}: known desks are `login` and `triage`"
        )),
    }
}

/// Answerable only by combining what the seats separately hold.
static LOGIN: Scenario = Scenario {
    id: "engineering",
    name: "Engineering",
    description: "Diagnose a regression from the seat that owns it.",
    task: "After the 0.9 release, the login flow rejects valid credentials. \
Nothing else regressed. Two things are needed: the one-line fix, and the \
regression test that would have caught this. They belong to different seats. \
Do the part that is yours, and hand the other part off -- you do not name who \
takes it, routing decides. No seat holds enough to diagnose alone either, so \
ask before you conclude.",
    seats: &[
        (
            "theory",
            "You are the structure specialist. Derive the exact shape of the \
problem and state which invariants must hold. You do not write fixes and you \
do not design tests; if the work needs either, it is not yours.",
            "You alone know: 0.9 replaced the password hashing library. Nobody \
else on the desk knows a library changed at all.",
        ),
        (
            "solver",
            "You are the implementation specialist. Say what the change itself \
would be, precisely enough that someone could make it. You do not design \
regression tests -- that is the verifier's -- and you do not research prior \
art.",
            "You alone know: the rehash migration was written but its job never \
ran in production. Nobody else knows a migration exists.",
        ),
        (
            "checker",
            "You are the adversarial verifier. You design the regression test that \
would catch this, and you attack the reading on the table. You do not write \
the fix itself.",
            "You alone know: accounts created after 0.9 log in fine; only older \
accounts fail. Nobody else has this observation.",
        ),
        (
            "lead",
            "You coordinate the desk. Reconcile what the seats hold and state the \
conclusion once it is supported.",
            "You know no facts of your own. You cannot answer without the others.",
        ),
        (
            "researcher",
            "You are the prior-art specialist. Say what is already known about \
this failure shape.",
            "You alone know: the new library writes a different hash prefix and \
its changelog says old hashes are not readable. Nobody else knows this.",
        ),
    ],
};

/// Built to fire what the login desk never did: a dispatcher whose only job
/// is three handoffs on a budget of two, so its first placed broadcast
/// completes it and its third is refused; and an askee whose answer turns on
/// a third seat, so it is tempted to `ask` inside the conversation.
static TRIAGE: Scenario = Scenario {
    id: "support",
    name: "Support",
    description: "Three overnight tickets, each owned by one seat.",
    task: "Three tickets came in overnight and they are one incident. (1) `GET \
/users/{id}` returns 500 for some users since yesterday's deploy. (2) The \
nightly `backfill-region` job shows as failed. (3) `test_users_have_region` \
is red on main. Each ticket belongs to one seat; the dispatcher owns none of \
them and hands each off -- you do not name who takes it, routing decides. No \
seat holds enough to close its ticket alone, so ask before you conclude, and \
say what you found when you do.",
    seats: &[
        (
            "dispatcher",
            "You triage. You hold the tickets and do no engineering yourself: \
hand each ticket off as its own piece of work, one per call, then stop. Do \
not diagnose and do not summarise.",
            "You know no facts of your own.",
        ),
        (
            "api",
            "You own the HTTP API. Say what the endpoint does wrong and the fix \
in the handler, precisely enough that someone could make it.",
            "You alone know: the 500 is a null dereference reading a user's \
`region`, which the handler assumes is set. Which users have it unset is \
the database seat's knowledge, not yours: ask `db` before you conclude.",
        ),
        (
            "db",
            "You own the schema and migrations. Say what the data looks like \
and why.",
            "You alone know: migration 0042 added `users.region` and its \
backfill is a separate job that fills rows in batches. Whether that job \
finished is `ops`' knowledge; you cannot say how many rows are unset without \
it, and you must have it before you answer anyone.",
        ),
        (
            "ops",
            "You run deploys and jobs. Say what ran, what did not, and why.",
            "You alone know: yesterday's deploy restarted the workers and \
killed `backfill-region` at 40%; it was never rerun. Nobody else knows the \
job was interrupted rather than broken.",
        ),
        (
            "qa",
            "You own the test suite. Say what a red test is actually asserting \
and whether the assertion is right.",
            "You alone know: `test_users_have_region` asserts every fixture \
user has a non-null `region`, and the fixtures were regenerated from a \
production snapshot taken after the deploy.",
        ),
    ],
};

/// How much of a reply that recorded nothing is shown in the log.
const REPLY_SHOWN: usize = 600;
const JEV_MODEL: &str = "jev-1.13.0";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;
/// Hard wall on turns: a chain that will not end is a finding, not a hang.
const TURN_WALL: u64 = 60;
/// Turns one conversation may take before it is concluded without an answer.
const CHILD_TURN_WALL: u64 = 6;

fn main() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(WORKER_STACK_BYTES)
        .build()?
        .block_on(run())
}

/// One desk row. The host owns the journal; this one is in memory.
#[derive(Clone, Debug)]
struct Row {
    sequence: Sequence,
    author: String,
    body: String,
    /// `None` is the open desk; `Some(root)` is the conversation rooted at
    /// that ask, which only its two seats read.
    thread: Option<Sequence>,
    /// On the open desk, a private row reaches one seat.
    only_for: Option<String>,
}

#[derive(Default)]
struct Journal {
    rows: Mutex<Vec<Row>>,
}

impl Journal {
    fn append(
        &self,
        author: &str,
        body: &str,
        thread: Option<Sequence>,
        only_for: Option<&str>,
    ) -> Sequence {
        let mut rows = self.rows.lock().expect("journal is not poisoned");
        let sequence = Sequence(rows.last().map_or(0, |row| row.sequence.0) + 1);
        rows.push(Row {
            sequence,
            author: author.to_owned(),
            body: body.to_owned(),
            thread,
            only_for: only_for.map(str::to_owned),
        });
        sequence
    }

    fn latest(&self) -> Sequence {
        self.rows
            .lock()
            .expect("journal is not poisoned")
            .last()
            .map_or(Sequence(0), |row| row.sequence)
    }

    /// What `seat` may read on the open desk above `after`: desk rows, and
    /// private rows addressed to it.
    fn desk_since(&self, seat: &str, after: Sequence) -> Vec<String> {
        self.rows
            .lock()
            .expect("journal is not poisoned")
            .iter()
            .filter(|row| row.sequence > after && row.thread.is_none())
            .filter(|row| row.only_for.as_deref().is_none_or(|only| only == seat))
            .map(render)
            .collect()
    }

    /// One conversation, whole: the ask that rooted it and every row in it.
    fn thread(&self, root: Sequence) -> Vec<String> {
        self.thread_since(root, Sequence(0))
    }

    fn thread_since(&self, root: Sequence, after: Sequence) -> Vec<String> {
        self.rows
            .lock()
            .expect("journal is not poisoned")
            .iter()
            .filter(|row| row.sequence > after)
            .filter(|row| row.sequence == root || row.thread == Some(root))
            .map(render)
            .collect()
    }

    fn all(&self) -> Vec<Row> {
        self.rows.lock().expect("journal is not poisoned").clone()
    }
}

fn render(row: &Row) -> String {
    format!("@{}: {}", row.author, row.body)
}

/// One open conversation: a thread of the desk, run as its own episode whose
/// participant is the seat asked, with the asker recorded here (ADR 0023). One
/// question, one answer: the seat asked concludes with `complete_episode`, and
/// its message is the answer; a follow-up is a further ask.
struct Child {
    root: Sequence,
    asker: String,
    askee: String,
    state: DriverState,
    turns: u64,
    /// The last thing the seat asked said in it: the conclusion, cross-posted.
    last_by_askee: Option<String>,
    /// Whether the seat asked has been told once that it has not answered.
    nudged: bool,
    /// Whether the seat asked took a turn in this wave.
    turned: bool,
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
    // those hang rather than fail. Stub them, the way `pe1006_hive` does.
    let backend = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {"id": "conducted", "email": "local@openhuman.local"}
        })))
        .mount(&backend)
        .await;

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
        offline_config()
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
            let report = match kind {
                RunnerKind::Embed => {
                    let runtime = host.runtime().await?;
                    let runner = host.embed(&runtime, 0).await?;
                    episode(runner, host.setup(kind, false)).await?
                }
                RunnerKind::Raw => {
                    host.prepare_raw()?;
                    let runner = host.raw(0).await?;
                    episode(runner, host.setup(kind, false)).await?
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
        format!(
            "{DESK_PREAMBLE}\n\n{}",
            standing_contract(
                tinyhivemind_tools::served_specs(),
                self.scenario.id,
                kind.how_to_call()
            )
        )
    }

    fn setup(&self, kind: RunnerKind, quiet: bool) -> Setup {
        Setup {
            scenario: self.scenario,
            kind,
            ids: self.ids.clone(),
            candidates: self.candidates.clone(),
            briefs: self.briefs.clone(),
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
            // `none()` leaves `mcp_boot` false, and without it the MCP
            // subsystem never dials the episode's endpoint: the seats are
            // never offered a tool at all, which reads exactly like a model
            // declining to call one.
            .services(episode_services())
            .backend_url(self.backend_url.clone())
            .provider(
                Provider::openai_compatible(
                    self.route.endpoint.clone(),
                    self.route.api_key.clone(),
                )
                .model(self.route.model.clone()),
            )
            // `mcp_call_tool` is a write as far as the gate is concerned, so a
            // read-only tier blocks the episode's own tools. The blast radius
            // is the allowlist, not the tier: three dispatchers and no shell.
            .access(Access::full())
            .build()
            .await?)
    }

    /// Seat the embed runner for one episode. `episode` keeps agent ids
    /// unique across a bench's episodes on the one runtime.
    async fn embed(&self, runtime: &Runtime, episode: u32) -> anyhow::Result<EmbedRunner> {
        EmbedRunner::seat(
            runtime,
            Arc::new(EpisodeTools::new(self.ids.iter().cloned())),
            &self.briefs,
            &self.contract(RunnerKind::Embed),
            &format!("{}-{episode}", self.run_id),
        )
        .await
    }

    /// A raw seat is resolved by the hosted turn against the process
    /// registry, so every seat is registered before a raw runner is seated.
    fn prepare_raw(&self) -> anyhow::Result<()> {
        let seats: Vec<(&str, &str)> = self
            .scenario
            .seats
            .iter()
            .map(|(id, role, _)| (*id, *role))
            .collect();
        RawRunner::prepare(&self.workspace, &seats)
    }

    async fn raw(&self, _episode: u32) -> anyhow::Result<RawRunner> {
        RawRunner::seat(
            Arc::new(EpisodeTools::new(self.ids.iter().cloned())),
            &self.briefs,
            &self.contract(RunnerKind::Raw),
            &self.config,
            &self.backend_url,
            &self.route,
            &self.workspace,
        )
        .await
    }
}

/// Both runners, `episodes` times each, offline, and one table.
///
/// The model is scripted, so nothing here is about answers: every seat
/// completes on its first turn. What differs between the arms is the host --
/// the road a tool call takes, what a turn costs to set up, and how much is
/// sent to the model -- and that is what the columns are.
///
/// `first` runs first. The arms share one process, so each begins with an
/// episode that is run and not counted; `TINYHIVEMIND_RUNNER=raw` puts the
/// raw arm first, and a difference that survives both orders is the
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
    // The raw seats first: the runtime reads the process registry as it
    // boots, and a definition written after that is never seen.
    host.prepare_raw()?;
    let runtime = host.runtime().await?;
    let order = match first {
        RunnerKind::Embed => [RunnerKind::Embed, RunnerKind::Raw],
        RunnerKind::Raw => [RunnerKind::Raw, RunnerKind::Embed],
    };
    for kind in order {
        println!("[bench] {} x{episodes}", kind.name());
        let runtime = &runtime;
        let run = move |index: u32| async move {
            match kind {
                RunnerKind::Embed => {
                    episode(host.embed(runtime, index).await?, host.setup(kind, true)).await
                }
                RunnerKind::Raw => episode(host.raw(index).await?, host.setup(kind, true)).await,
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
        "\nturns/ep: seat turns the loop ran; waves/ep: rounds it took. requests/turn: model calls per turn, including any \
         discovery. KiB/turn: request bytes to the model. tool rtt: from the model emitting a \
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
    briefs: BTreeMap<String, String>,
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
/// hive and the driver as itself: the loop never names it.
async fn episode<R: SeatRunner>(runner: R, setup: Setup) -> anyhow::Result<Report> {
    let Setup {
        scenario,
        kind,
        ids,
        candidates,
        briefs,
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
    let hive = OpenHumanHive::new(
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
    let journal = Journal::default();
    journal.append("operator", scenario.task, None, None);

    // The door route: who starts. Everyone is seated so a handoff can reach
    // any seat; the ones the route passed over are completed at once -- idle,
    // and reopened by any broadcast that finds them.
    let door = RoutingRequest {
        message: scenario.task.to_owned(),
        source: RoutingSource::DeskMessage,
        conversation: ConversationRef {
            id: desk_id.into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: Some("Diagnose a regression from the seat that owns it.".to_owned()),
        thread_context: Vec::new(),
        candidates: candidates.clone(),
        roster_version: 1,
        policy: policy(4),
    };
    let plan = route_message(primary, None, &door, None, fallback).await;
    let starters = match &plan {
        RoutingPlan::One { responder_id, .. } | RoutingPlan::Fallback { responder_id, .. } => {
            vec![responder_id.clone()]
        }
        RoutingPlan::Hive {
            primary_id,
            invited_ids,
            ..
        } => std::iter::once(primary_id.clone())
            .chain(invited_ids.iter().cloned())
            .collect(),
        RoutingPlan::Clarify { .. } => {
            eprintln!("[door] routing asked for clarification; lead owns it");
            vec![fallback.to_owned()]
        }
    };
    println!("[door] starts: {}", starters.join(", "));

    let mut episode = CompletionEpisodeState::opened(
        Conversation {
            desk_id: desk_id.into(),
            desk_name: scenario.name.into(),
            thread_root: None,
        },
        Sequence(0),
        ids.iter().map(String::as_str),
    )?;
    for id in &ids {
        if !starters.contains(id) {
            episode = apply_completion(&episode, id, Sequence(1))?;
        }
    }

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

    let mut state = driver.start(episode)?;
    let mut children: BTreeMap<Sequence, Child> = BTreeMap::new();
    // Concluded conversations, kept whole for the context of the seats that
    // had them: `(root, asker, askee, transcript)`, and how many each seat has
    // already been shown.
    let mut concluded: Vec<(Sequence, String, String, Vec<String>)> = Vec::new();
    let mut shown: BTreeMap<String, usize> = BTreeMap::new();
    let mut desk_nudged: BTreeMap<String, Sequence> = BTreeMap::new();
    let mut turns = 0_u64;
    let mut waves = 0_u64;
    let mut discharged = 0_u64;
    let settled = 'episode: loop {
        if state.quiescent() && children.is_empty() {
            break Ok(());
        }
        waves += 1;

        // A seat that holds open desk work, ran for it, and has been shown
        // everything is stalled: nothing will wake it. Once per assignment it
        // is told, and owed one more turn.
        for seat_id in state.stalled() {
            let assigned_at = state
                .episode()
                .participants
                .iter()
                .find(|participant| participant.agent_id == seat_id)
                .and_then(|participant| participant.open())
                .map(|record| record.assigned_at);
            if desk_nudged.get(&seat_id) == assigned_at.as_ref() {
                continue;
            }
            if let Some(at) = assigned_at {
                desk_nudged.insert(seat_id.clone(), at);
            }
            eprintln!("[nudged] @{seat_id} on the desk: stalled with open work");
            journal.append(
                "desk",
                "you hold open work and nothing new has arrived. Call `complete_episode` \
                 with what you have, or `broadcast` the part that is another seat's. A reply \
                 without a tool call records nothing.",
                None,
                Some(&seat_id),
            );
            state.owe_turn(&seat_id);
        }
        for child in children.values_mut() {
            child.turned = false;
        }

        // One turn per seat per wave, and conversations first: a conversation
        // is what unblocks a desk turn, so it goes ahead of it.
        let mut taken: BTreeSet<String> = BTreeSet::new();
        let mut jobs: Vec<TurnJob> = Vec::new();
        for child in children.values_mut() {
            // Collected first: the round borrows the state it was proposed
            // from, and preparing a turn reports delivery into that state.
            let seats: Vec<String> = driver
                .pending_round(&child.state)?
                .agents()
                .iter()
                .map(|pending| pending.hive_agent_id.to_owned())
                .collect();
            for seat_id in seats {
                if !taken.insert(seat_id.clone()) {
                    continue;
                }
                let through = child
                    .state
                    .seen()
                    .delivered_through
                    .get(&seat_id)
                    .copied()
                    .unwrap_or(child.root);
                let rows = journal.thread_since(child.root, through);
                runner.open(
                    &seat_id,
                    journal.thread(child.root),
                    Dispatch {
                        chat: desk_id.into(),
                        parent: Some(child.root.0.to_string()),
                    },
                );
                child.state.delivered(&seat_id, journal.latest());
                child.state.turn_started(&seat_id);
                child.turns += 1;
                child.turned = true;
                let other = if seat_id == child.asker {
                    child.askee.clone()
                } else {
                    child.asker.clone()
                };
                let brief = EpisodeBrief::for_turn(
                    &child.state,
                    desk_id,
                    &seat_id,
                    Channel::Thread {
                        root: child.root,
                        other: other.clone(),
                        opened_it: seat_id == child.asker,
                    },
                    rows,
                    Vec::new(),
                );
                let prompt = format!(
                    "## The desk\n{DESK_PREAMBLE}\n\n## Who you are\n{}\n\n{}",
                    briefs[&seat_id],
                    brief.render()
                );
                jobs.push(runner.turn(seat_id, Lane::Thread(child.root), prompt));
            }
        }
        let seats: Vec<String> = driver
            .pending_round(&state)?
            .agents()
            .iter()
            .map(|pending| pending.hive_agent_id.to_owned())
            .collect();
        for seat_id in seats {
            if !taken.insert(seat_id.clone()) {
                continue;
            }
            let through = state
                .seen()
                .delivered_through
                .get(&seat_id)
                .copied()
                .unwrap_or(Sequence(0));
            let rows = journal.desk_since(&seat_id, through);
            runner.open(
                &seat_id,
                rows.clone(),
                Dispatch {
                    chat: desk_id.into(),
                    parent: None,
                },
            );
            state.delivered(&seat_id, journal.latest());
            state.turn_started(&seat_id);
            // The conversations this seat had: concluded since it last spoke,
            // shown whole once, and any still in progress -- its shared
            // context across every channel it is in.
            let cursor = shown.entry(seat_id.clone()).or_insert(0);
            let mut views: Vec<ConversationView> = concluded[*cursor..]
                .iter()
                .filter(|(_, asker, askee, _)| *asker == seat_id || *askee == seat_id)
                .map(|(root, asker, askee, transcript)| ConversationView {
                    root: *root,
                    other: if *asker == seat_id {
                        askee.clone()
                    } else {
                        asker.clone()
                    },
                    opened_it: *asker == seat_id,
                    transcript: transcript.clone(),
                    concluded: true,
                })
                .collect();
            *cursor = concluded.len();
            views.extend(
                children
                    .values()
                    .filter(|child| child.asker == seat_id || child.askee == seat_id)
                    .map(|child| ConversationView {
                        root: child.root,
                        other: if child.asker == seat_id {
                            child.askee.clone()
                        } else {
                            child.asker.clone()
                        },
                        opened_it: child.asker == seat_id,
                        transcript: journal.thread(child.root),
                        concluded: false,
                    }),
            );
            let brief =
                EpisodeBrief::for_turn(&state, desk_id, &seat_id, Channel::Desk, rows, views);
            // What the host owns first; what the episode knows after.
            let prompt = format!(
                "## The desk\n{DESK_PREAMBLE}\n\n## Who you are\n{}\n\n{}",
                briefs[&seat_id],
                brief.render()
            );
            jobs.push(runner.turn(seat_id, Lane::Desk, prompt));
        }
        if jobs.is_empty() {
            // Nothing is due anywhere. A conversation nobody will continue is
            // concluded without an answer; a desk with open work and nobody
            // owed a turn is stalled.
            let stuck: Vec<Sequence> = children.keys().copied().collect();
            if stuck.is_empty() {
                break Err(anyhow::anyhow!(
                    "episode stalled with {:?} holding open work",
                    state.stalled()
                ));
            }
            for root in stuck {
                let child = children.remove(&root).expect("listed");
                state = conclude(&driver, &journal, &state, child, true, &mut concluded).await?;
            }
            continue;
        }
        let outcomes = futures::future::join_all(jobs).await;

        // Everything the wave said, sorted into the channel it belongs to.
        // A broadcast made inside a conversation is desk work: the seat found
        // something for someone else while talking, and refusing it there
        // cost a live run both of its deliverables.
        let mut desk_events: Vec<(String, Utterance)> = Vec::new();
        let mut thread_events: Vec<(Sequence, String, Utterance)> = Vec::new();
        for (seat_id, lane, outcome) in outcomes {
            turns += 1;
            let where_ = match lane {
                Lane::Desk => String::new(),
                Lane::Thread(root) => format!(" in thread {}", root.0),
            };
            match &outcome {
                Some(Ok(reply)) => eprintln!(
                    "[turn] @{seat_id}{where_} replied ({} chars)",
                    reply.chars().count()
                ),
                Some(Err(error)) => eprintln!("[turn] @{seat_id}{where_} failed: {error}"),
                None => eprintln!("[turn] @{seat_id}{where_} timed out"),
            }
            // Close the turn first: the record refuses a call on a closed
            // turn, so nothing can land after this point is read.
            let events = runner.close(&seat_id);
            for refused in runner.tools().drain_refusals(&seat_id) {
                eprintln!(
                    "[refused] @{seat_id}{where_} `{}`: {}",
                    refused.tool, refused.reason
                );
            }
            if events.is_empty() {
                eprintln!("[no tool call] @{seat_id}{where_}");
                // What the seat wrote instead: the only trace of a refusal
                // it read, or of a deliverable it typed rather than recorded.
                if let Some(Ok(reply)) = &outcome {
                    let shown: String = reply.chars().take(REPLY_SHOWN).collect();
                    let cut = if reply.chars().count() > REPLY_SHOWN {
                        " [...]"
                    } else {
                        ""
                    };
                    eprintln!("    {}{cut}", shown.replace('\n', "\n    "));
                }
            }
            for event in events {
                let ToolCall::Speak(utterance) = event.call else {
                    continue;
                };
                match (lane, &utterance) {
                    (Lane::Desk, _)
                    | (Lane::Thread(_), Utterance::Broadcast { .. } | Utterance::Ask { .. }) => {
                        desk_events.push((seat_id.clone(), utterance));
                    }
                    (Lane::Thread(root), _) => {
                        thread_events.push((root, seat_id.clone(), utterance))
                    }
                }
            }
        }

        for (root, seat_id, utterance) in thread_events {
            let Some(child) = children.get_mut(&root) else {
                continue;
            };
            if !matches!(
                utterance,
                Utterance::Post { .. } | Utterance::CompleteEpisode { .. }
            ) {
                // Only `dm` can reach here, and it is not served.
                continue;
            }
            let sequence = journal.append(&seat_id, &describe(&utterance), Some(root), None);
            if seat_id == child.askee {
                child.last_by_askee = Some(utterance.message().to_owned());
            }
            let committed = CommittedUtterance {
                author_id: seat_id.clone(),
                sequence,
                utterance,
            };
            match driver.apply_committed(&child.state, committed, None).await {
                Ok(transition) => child.state = transition.state,
                Err(Error::UndeliveredAssignment { .. }) => {
                    eprintln!("[refused] @{seat_id} in thread {}: not yet shown", root.0);
                }
                Err(error) => break 'episode Err(error.into()),
            }
        }

        // The seat asked took its turn and the conversation is not over: it
        // did not answer, whatever it did instead. Once, it is told so and
        // owed one more turn; a second silence stands.
        for child in children.values_mut() {
            if child.turned && !child.state.quiescent() && !child.nudged {
                child.nudged = true;
                journal.append(
                    "desk",
                    "the seat that asked you is waiting: answer with `complete_episode`, and its \
                     message is your answer. If you need another seat first, say so in that answer.",
                    Some(child.root),
                    None,
                );
                child.state.owe_turn(&child.askee);
                eprintln!("[nudged] @{} in thread {}", child.askee, child.root.0);
            }
        }

        for (seat_id, utterance) in desk_events {
            // An ask is private to the seat it asks and roots a conversation;
            // everything else is the desk's.
            let asked = utterance.asks().map(str::to_owned);
            let sequence = journal.append(&seat_id, &describe(&utterance), None, asked.as_deref());
            let is_broadcast = utterance.broadcasting();
            let held = holds(&state, &seat_id);
            let committed = CommittedUtterance {
                author_id: seat_id.clone(),
                sequence,
                utterance,
            };
            match driver
                .apply_committed(&state, committed, Some(routing))
                .await
            {
                Ok(transition) => {
                    let mut routed = false;
                    for action in &transition.actions {
                        match action {
                            HostAction::RunAgents { agent_ids, .. } => {
                                routed = true;
                                println!("[broadcast] @{seat_id} -> {}", agent_ids.join(", "));
                            }
                            // For an ask, this is the signal to open the
                            // conversation: a thread of the desk rooted at the
                            // ask row, with the two as its seats.
                            HostAction::DeliverDm { .. } => {
                                if let Some(askee) = &asked {
                                    let child_state =
                                        driver.start(CompletionEpisodeState::opened(
                                            Conversation {
                                                desk_id: desk_id.into(),
                                                desk_name: scenario.name.into(),
                                                thread_root: Some(sequence),
                                            },
                                            sequence,
                                            [askee.as_str()],
                                        )?)?;
                                    println!(
                                        "[ask] @{seat_id} opened a conversation with @{askee} (thread {})",
                                        sequence.0
                                    );
                                    children.insert(
                                        sequence,
                                        Child {
                                            root: sequence,
                                            asker: seat_id.clone(),
                                            askee: askee.clone(),
                                            state: child_state,
                                            turns: 0,
                                            last_by_askee: None,
                                            nudged: false,
                                            turned: false,
                                        },
                                    );
                                }
                            }
                            HostAction::DeliverHandoff { agent_id, handoff } => {
                                println!(
                                    "[handoff] -> @{agent_id} (queued from @{})",
                                    handoff.from
                                );
                                journal.append(
                                    "desk",
                                    &format!("handoff from @{}: {}", handoff.from, handoff.body),
                                    None,
                                    Some(agent_id),
                                );
                            }
                        }
                    }
                    if is_broadcast && held && !holds(&transition.state, &seat_id) {
                        eprintln!("[completed] @{seat_id} by its broadcast");
                    }
                    if is_broadcast && !routed {
                        println!(
                            "[unplaced] @{seat_id}'s broadcast fits no seat; it keeps the work"
                        );
                        journal.append(
                            "desk",
                            "nobody on this desk can take that; the work stays with you. Do what \
                             you can with what the desk holds, or complete with what you have.",
                            None,
                            Some(&seat_id),
                        );
                    }
                    state = transition.state;
                }
                Err(Error::AwaitingReply { waiting_on, .. }) => {
                    eprintln!(
                        "[refused] @{seat_id} may not complete: in conversation with {waiting_on:?}"
                    );
                    journal.append(
                        "desk",
                        &format!(
                            "your completion was refused: your conversation with {} has not \
                             concluded. Its outcome reaches you on a later turn; complete after it \
                             does.",
                            waiting_on.join(", ")
                        ),
                        None,
                        Some(&seat_id),
                    );
                }
                Err(Error::UndeliveredAssignment { assigned_at, .. }) => {
                    eprintln!(
                        "[refused] @{seat_id} completed before seeing its assignment at {}",
                        assigned_at.0
                    );
                    journal.append(
                        "desk",
                        &format!(
                            "you were handed new work at sequence {} while you were speaking; it \
                             is in your next messages. Your completion applied to nothing.",
                            assigned_at.0
                        ),
                        None,
                        Some(&seat_id),
                    );
                }
                Err(Error::BudgetSpent { .. }) => {
                    eprintln!(
                        "[refused] @{seat_id} has spent its broadcast budget; it keeps the work"
                    );
                    discharged += 1;
                    let sequence =
                        journal.append(&seat_id, "budget spent; keeping the work", None, None);
                    let transition = driver
                        .apply_committed(
                            &state,
                            CommittedUtterance {
                                author_id: seat_id.clone(),
                                sequence,
                                utterance: Utterance::CompleteEpisode {
                                    message: "budget spent; keeping the work".into(),
                                },
                            },
                            None,
                        )
                        .await?;
                    state = transition.state;
                }
                Err(error) => break 'episode Err(error.into()),
            }
        }

        // Conversations that ended this wave, or ran past their wall, conclude:
        // their outcome is cross-posted to the asker, which releases its hold.
        let over: Vec<Sequence> = children
            .iter()
            .filter(|(_, child)| child.state.quiescent() || child.turns >= CHILD_TURN_WALL)
            .map(|(root, _)| *root)
            .collect();
        for root in over {
            let child = children.remove(&root).expect("listed");
            let forced = !child.state.quiescent();
            state = conclude(&driver, &journal, &state, child, forced, &mut concluded).await?;
        }
        if turns >= TURN_WALL {
            break Err(anyhow::anyhow!("turn wall of {TURN_WALL} reached"));
        }
    };

    println!(
        "turns {turns} | routes {} | waves {waves} | discharged {discharged} | settled {} | conversations {}",
        router
            .as_ref()
            .map_or(0, |r| r.calls.load(Ordering::SeqCst)),
        state.episode().settled(),
        concluded.len()
    );
    for row in journal.all().iter().filter(|_| !quiet) {
        let scope = match (row.thread, row.only_for.as_deref()) {
            (Some(root), _) => format!(" (thread {})", root.0),
            (None, Some(only)) => format!(" (to @{only})"),
            (None, None) => String::new(),
        };
        println!(
            "  {:>3}  @{}{scope}: {}",
            row.sequence.0, row.author, row.body
        );
    }
    settled?;
    // Offline, the run is a proof and says so: the scripted seat's tool call
    // must have become a desk row -- natively through the belt, or over the
    // wire through the server -- and either way through the record.
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
                    RunnerKind::Embed => "`mcp_call_tool`",
                    RunnerKind::Raw => "native",
                }
            );
        }
    }
    drop(runner);
    Ok(Report {
        turns,
        waves,
        wall: started.elapsed(),
    })
}

/// A config that touches nothing on this machine: no local runtime, no
/// python, no embedding endpoint. The same neutralisation `main.rs` uses.
fn offline_config() -> RuntimeConfig {
    let mut config = RuntimeConfig::default();
    config.local_ai.runtime_enabled = false;
    config.runtime_python.enabled = false;
    config.memory_tree.spacy_enabled = false;
    config.memory_tree.embedding_endpoint = None;
    config.memory_tree.embedding_model = None;
    config.memory_tree.embedding_strict = false;
    config
}

/// Conclude one conversation: cross-post its outcome to the asker as a
/// private message from the seat asked. That row is what releases the
/// asker's hold and wakes it; the whole conversation reaches it in its next
/// prompt. `forced` is a conversation that ran out of turns.
async fn conclude<A: BoundAgent>(
    driver: &CompletionDriver<'_, A>,
    journal: &Journal,
    state: &DriverState,
    child: Child,
    forced: bool,
    concluded: &mut Vec<(Sequence, String, String, Vec<String>)>,
) -> anyhow::Result<DriverState> {
    let transcript = journal.thread(child.root);
    let outcome = if forced {
        "the conversation did not conclude in time; take what was said and proceed".to_owned()
    } else {
        child
            .last_by_askee
            .clone()
            .unwrap_or_else(|| "concluded".to_owned())
    };
    println!(
        "[concluded] thread {} between @{} and @{}{}",
        child.root.0,
        child.asker,
        child.askee,
        if forced {
            " (nothing due, or out of turns)"
        } else {
            ""
        }
    );
    let sequence = journal.append(
        &child.askee,
        &format!(
            "concluded our conversation (thread {}): {outcome}",
            child.root.0
        ),
        None,
        Some(&child.asker),
    );
    let transition = driver
        .apply_committed(
            state,
            CommittedUtterance {
                author_id: child.askee.clone(),
                sequence,
                utterance: Utterance::Dm {
                    to: vec![child.asker.clone()],
                    message: outcome,
                },
            },
            None,
        )
        .await?;
    concluded.push((child.root, child.asker, child.askee, transcript));
    Ok(transition.state)
}

/// How a row reads on the desk.
fn describe(utterance: &Utterance) -> String {
    match utterance {
        Utterance::Post { message } => message.clone(),
        Utterance::Broadcast { message } => format!("BROADCAST: {message}"),
        Utterance::Ask { to, message } => format!("asks @{to}: {message}"),
        Utterance::Dm { message, .. } => message.clone(),
        Utterance::CompleteEpisode { message } => format!("COMPLETE: {message}"),
    }
}

/// Nothing running but the one subsystem the episode's tools arrive through.
fn episode_services() -> ServiceSet {
    let mut services = ServiceSet::none();
    services.mcp_boot = true;
    services
}

fn required(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|_| anyhow::anyhow!("{name} must be set for a live run"))
}

/// Whether `seat` holds an open assignment on the desk.
fn holds(state: &DriverState, seat: &str) -> bool {
    state
        .episode()
        .participants
        .iter()
        .any(|participant| participant.agent_id == seat && participant.open().is_some())
}

fn candidate(id: &str, role: &str) -> RouteCandidate {
    RouteCandidate {
        id: id.to_owned(),
        label: id.to_owned(),
        role: Some(role.to_owned()),
        description: Some(role.to_owned()),
        capabilities: Vec::new(),
        learned_topics: Vec::new(),
        available: true,
    }
}

fn policy(round_width: usize) -> RoutingPolicy {
    RoutingPolicy {
        minimum_confidence: probability(350_000),
        high_impact_minimum_confidence: probability(800_000),
        clarification_threshold: probability(850_000),
        high_impact_threshold: probability(700_000),
        round_width,
        choice_option_limit: 8,
    }
}

fn probability(parts: u32) -> Probability {
    Probability::new(parts).expect("parts within scale")
}
