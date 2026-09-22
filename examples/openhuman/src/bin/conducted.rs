//! One completion-driven episode, conducted over real OpenHuman agents.
//!
//! The point of this binary is not the task. It is the shape of the host:
//! the room's tools are served by `tinyhivemind-mcp`, the loop is
//! `tinyhivemind-openhuman`'s driver stepped the way a host steps it --
//! propose a round, run it, commit what it said, report delivery, repeat until
//! quiescent -- and what is written here is only what a host owns: agents,
//! a journal, sessions, and the prompt a turn is shown.
//!
//! ```sh
//! set -a; . ~/.config/tinyhivemind/live.env; set +a
//! TINYHIVEMIND_LIVE_OPENROUTER=1 cargo run --manifest-path examples/openhuman/Cargo.toml --bin conducted
//! ```

mod conducted {
    pub mod jev;
}

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use conducted::jev::LiveJev;
use openhuman_core::agent::registry::types::{
    AgentRegistryEntry, AgentRegistrySource, AgentSubagentPolicy,
};
use openhuman_embed::{
    Access, Agent, AgentDefinitionSpec, AgentSpec, McpServer, Provider, Runtime, RuntimeConfig,
    ServiceSet, ToolScopeSpec, Workspace,
};
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
use tinyhivemind_mcp::{Dispatch, EpisodeTools, serve};
use tinyhivemind_openhuman::{
    AgentBinding, BroadcastRouting, Channel, CommittedUtterance, CompletionDriver,
    ConversationView, DriverState, EpisodeBrief, Error, HiveGraph, HostAction, OpenHumanHive,
    standing_contract,
};
use tinyhivemind_typesafe::JevRouter;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// What the host says about the desk, before the episode's own contract.
const DESK_PREAMBLE: &str = "\
You are one seat on a desk. You have no codebase, shell or filesystem -- only
the desk's messages and your own judgement. Never ask for permission and never
wait to be told to continue; nobody will answer.";

/// The desk, and what each seat privately knows: a hidden profile, so no seat
/// can answer alone and the tools are necessary rather than available.
const SEATS: [(&str, &str, &str); 5] = [
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
];

/// Answerable only by combining what the seats separately hold.
const TASK: &str = "After the 0.9 release, the login flow rejects valid credentials. \
Nothing else regressed. Two things are needed: the one-line fix, and the \
regression test that would have caught this. They belong to different seats. \
Do the part that is yours, and hand the other part off -- you do not name who \
takes it, routing decides. No seat holds enough to diagnose alone either, so \
ask before you conclude.";

const DESK_ID: &str = "engineering";
const JEV_MODEL: &str = "jev-1.13.0";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;
const TURN_TIMEOUT: Duration = Duration::from_secs(300);
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

/// One open conversation: a thread of the desk, run as its own episode with
/// the asker and the seat asked as its participants (ADR 0023).
struct Child {
    root: Sequence,
    asker: String,
    askee: String,
    state: DriverState,
    turns: u64,
    /// The last thing the seat asked said in it: the conclusion, cross-posted.
    last_by_askee: Option<String>,
}

/// Where a turn is running, for the host's own bookkeeping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lane {
    Desk,
    Thread(Sequence),
}

/// A turn's reply, once it is back: `None` timed out.
type TurnResult = Option<Result<String, String>>;
type TurnJob = std::pin::Pin<Box<dyn Future<Output = (String, Lane, TurnResult)> + Send>>;

fn spawn_turn(agent: Agent, session: String, seat: String, lane: Lane, prompt: String) -> TurnJob {
    Box::pin(async move {
        let result =
            match tokio::time::timeout(TURN_TIMEOUT, agent.turn(prompt).session(&session).send())
                .await
            {
                Ok(Ok(outcome)) => Some(Ok(outcome.reply)),
                Ok(Err(error)) => Some(Err(error.to_string())),
                Err(_) => None,
            };
        (seat, lane, result)
    })
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
    let _ = env_logger::builder().is_test(false).try_init();
    if std::env::var_os("TINYHIVEMIND_LIVE_OPENROUTER").is_none() {
        eprintln!(
            "conducted: set TINYHIVEMIND_LIVE_OPENROUTER=1, OPENROUTER_API_KEY,\n\
             OPENROUTER_MODEL and TYPESAFE_API_KEY to run a live episode.\n\
             It makes one model call per agent turn and one Jev call per route."
        );
        return Ok(());
    }
    let key = required("OPENROUTER_API_KEY")?;
    let model = required("OPENROUTER_MODEL")?;

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
    std::fs::create_dir_all(&run_dir)?;

    let mut config = RuntimeConfig::load_or_init().await?;
    config.agent.max_tool_iterations = 6;
    config.default_temperature = 0.0;

    let runtime = Runtime::builder()
        .config(config)
        .workspace(Workspace::dir(run_dir.join("openhuman-runtime")))
        // `none()` leaves `mcp_boot` false, and without it the MCP subsystem
        // never dials the episode's endpoint: the seats are never offered a
        // tool at all, which reads exactly like a model declining to call one.
        .services(episode_services())
        .backend_url(backend.uri())
        .provider(Provider::openai_compatible(OPENROUTER, key).model(model))
        // `mcp_call_tool` is a write as far as the gate is concerned, so a
        // read-only tier blocks the episode's own tools. The blast radius is
        // the allowlist below, not the tier: three dispatchers and no shell.
        .access(Access::full())
        .build()
        .await?;

    // The room's tools, served by the crate that owns them. One endpoint per
    // seat: identity is the URL dialled, never a field filled in.
    let ids: Vec<String> = SEATS.iter().map(|(id, _, _)| (*id).to_owned()).collect();
    let tools = Arc::new(EpisodeTools::new(ids.iter().cloned()));
    let server = serve(Arc::clone(&tools)).await?;

    let briefs: BTreeMap<String, String> = SEATS
        .iter()
        .map(|(id, role, known)| {
            (
                (*id).to_owned(),
                format!("{role}\n\n## What you privately know\n{known}"),
            )
        })
        .collect();
    // The standing contract comes from the vocabulary, for exactly the tools
    // the server serves; the host adds its one sentence on the mechanics.
    let contract = format!(
        "{DESK_PREAMBLE}\n\n{}",
        standing_contract(
            tinyhivemind_mcp::served_specs(),
            DESK_ID,
            "Use `mcp_call_tool` with `server: \"episode\"`.",
        )
    );
    let mut agents: BTreeMap<String, Agent> = BTreeMap::new();
    for (id, _, _) in SEATS {
        let agent = seat(
            &runtime,
            id,
            &briefs[id],
            &contract,
            &run_id,
            &server.endpoint(id),
        )?;
        eprintln!(
            "[seat] {id}: {} mcp server(s) registered",
            agent.config().mcp_client.servers.len()
        );
        agents.insert((*id).to_owned(), agent);
    }

    let candidates: Vec<RouteCandidate> = SEATS
        .iter()
        .map(|(id, role, _)| candidate(id, role))
        .collect();
    let seated: BTreeSet<&str> = agents.keys().map(String::as_str).collect();
    let advertised: BTreeSet<&str> = candidates.iter().map(|c| c.id.as_str()).collect();
    anyhow::ensure!(
        seated == advertised,
        "roster drift: instantiated {seated:?} but advertising {advertised:?}"
    );
    let hive = OpenHumanHive::new(
        HiveGraph::new(
            Desk {
                id: DESK_ID.into(),
                name: "Engineering".into(),
                description: Some("Diagnose a regression from the seat that owns it.".into()),
                members: ids.clone(),
                responder_mode: ResponderMode::Auto,
            },
            candidates.clone(),
        ),
        agents
            .iter()
            .map(|(id, agent)| AgentBinding::new(id.clone(), agent.clone()))
            .collect(),
    )?;

    let router = Counted {
        inner: JevRouter::with_model(LiveJev::from_env()?, JEV_MODEL),
        calls: AtomicU64::new(0),
    };
    let journal = Journal::default();
    journal.append("operator", TASK, None, None);

    // The door route: who starts. Everyone is seated so a handoff can reach
    // any seat; the ones the route passed over are completed at once -- idle,
    // and reopened by any broadcast that finds them.
    let door = RoutingRequest {
        message: TASK.to_owned(),
        source: RoutingSource::DeskMessage,
        conversation: ConversationRef {
            id: DESK_ID.into(),
            kind: ConversationKind::Desk,
            thread_root: None,
        },
        desk_purpose: Some("Diagnose a regression from the seat that owns it.".to_owned()),
        thread_context: Vec::new(),
        candidates: candidates.clone(),
        roster_version: 1,
        policy: policy(4),
    };
    let plan = route_message(Some(&router), None, &door, None, "lead").await;
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
            vec!["lead".to_owned()]
        }
    };
    println!("[door] starts: {}", starters.join(", "));

    let mut episode = CompletionEpisodeState::opened(
        Conversation {
            desk_id: DESK_ID.into(),
            desk_name: "Engineering".into(),
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
        primary: Some(&router),
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
    let mut turns = 0_u64;
    let mut waves = 0_u64;
    let mut discharged = 0_u64;
    let settled = 'episode: loop {
        if state.quiescent() && children.is_empty() {
            break Ok(());
        }
        waves += 1;

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
                // The asker's question is the thread's first row. Until the
                // seat asked has said anything, the asker has nothing to add.
                if child.state.revision() == 0 && seat_id == child.asker {
                    continue;
                }
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
                tools.window(&seat_id, journal.thread(child.root));
                tools.register(
                    &seat_id,
                    Dispatch {
                        chat: DESK_ID.into(),
                        parent: Some(child.root.0.to_string()),
                    },
                );
                child.state.delivered(&seat_id, journal.latest());
                child.state.turn_started(&seat_id);
                child.turns += 1;
                let other = if seat_id == child.asker {
                    child.askee.clone()
                } else {
                    child.asker.clone()
                };
                let brief = EpisodeBrief::for_turn(
                    &child.state,
                    DESK_ID,
                    &seat_id,
                    Channel::Thread {
                        root: child.root,
                        other: other.clone(),
                        opened_it: seat_id == child.asker,
                    },
                    rows,
                    Vec::new(),
                );
                let prompt = format!("## Who you are\n{}\n\n{}", briefs[&seat_id], brief.render());
                jobs.push(spawn_turn(
                    agents[&seat_id].clone(),
                    format!("conducted-{run_id}:{seat_id}"),
                    seat_id,
                    Lane::Thread(child.root),
                    prompt,
                ));
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
            tools.window(&seat_id, rows.clone());
            tools.register(
                &seat_id,
                Dispatch {
                    chat: DESK_ID.into(),
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
                EpisodeBrief::for_turn(&state, DESK_ID, &seat_id, Channel::Desk, rows, views);
            // What the host owns first; what the episode knows after.
            let prompt = format!("## Who you are\n{}\n\n{}", briefs[&seat_id], brief.render());
            jobs.push(spawn_turn(
                agents[&seat_id].clone(),
                format!("conducted-{run_id}:{seat_id}"),
                seat_id,
                Lane::Desk,
                prompt,
            ));
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

        for (seat_id, lane, outcome) in outcomes {
            tools.clear(&seat_id);
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
            let events = tools.drain(&seat_id);
            if events.is_empty() {
                eprintln!("[no tool call] @{seat_id}{where_}");
            }
            match lane {
                Lane::Thread(root) => {
                    let Some(child) = children.get_mut(&root) else {
                        continue;
                    };
                    for event in events {
                        let ToolCall::Speak(utterance) = event.call else {
                            continue;
                        };
                        if !matches!(
                            utterance,
                            Utterance::Post { .. } | Utterance::CompleteEpisode { .. }
                        ) {
                            journal.append(
                                "desk",
                                "`ask` and `broadcast` are not available inside a conversation. \
                                 Answer with `post`, or `complete_episode` to conclude your side.",
                                Some(root),
                                None,
                            );
                            continue;
                        }
                        let sequence =
                            journal.append(&seat_id, &describe(&utterance), Some(root), None);
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
                                eprintln!(
                                    "[refused] @{seat_id} in thread {}: not yet shown",
                                    root.0
                                );
                            }
                            Err(error) => break 'episode Err(error.into()),
                        }
                    }
                }
                Lane::Desk => {
                    for event in events {
                        let ToolCall::Speak(utterance) = event.call else {
                            continue;
                        };
                        // An ask is private to the seat it asks and roots a
                        // conversation; everything else is the desk's.
                        let asked = utterance.asks().map(str::to_owned);
                        let sequence =
                            journal.append(&seat_id, &describe(&utterance), None, asked.as_deref());
                        let is_broadcast = utterance.broadcasting();
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
                                            println!(
                                                "[broadcast] @{seat_id} -> {}",
                                                agent_ids.join(", ")
                                            );
                                        }
                                        // For an ask, this is the signal to open the
                                        // conversation: a thread of the desk rooted
                                        // at the ask row, with the two as its seats.
                                        HostAction::DeliverDm { .. } => {
                                            if let Some(askee) = &asked {
                                                let child_state = driver.start(
                                                    CompletionEpisodeState::opened(
                                                        Conversation {
                                                            desk_id: DESK_ID.into(),
                                                            desk_name: "Engineering".into(),
                                                            thread_root: Some(sequence),
                                                        },
                                                        sequence,
                                                        [seat_id.as_str(), askee.as_str()],
                                                    )?,
                                                )?;
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
                                                &format!(
                                                    "handoff from @{}: {}",
                                                    handoff.from, handoff.body
                                                ),
                                                None,
                                                Some(agent_id),
                                            );
                                        }
                                    }
                                }
                                if is_broadcast && !routed {
                                    println!(
                                        "[unplaced] @{seat_id}'s broadcast fits no seat; it keeps the work"
                                    );
                                    journal.append(
                                        "desk",
                                        "nobody on this desk can take that; the work stays with you. \
                                         Do what you can with what the desk holds, or complete with \
                                         what you have.",
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
                                        "your completion was refused: your conversation with {} has \
                                         not concluded. Its outcome reaches you on a later turn; \
                                         complete after it does.",
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
                                        "you were handed new work at sequence {} while you were \
                                         speaking; it is in your next messages. Your completion \
                                         applied to nothing.",
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
                                let sequence = journal.append(
                                    &seat_id,
                                    "budget spent; keeping the work",
                                    None,
                                    None,
                                );
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
                }
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
        router.calls.load(Ordering::SeqCst),
        state.episode().settled(),
        concluded.len()
    );
    for row in journal.all() {
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
    drop(server);
    settled
}

/// Conclude one conversation: cross-post its outcome to the asker as a
/// private message from the seat asked. That row is what releases the
/// asker's hold and wakes it; the whole conversation reaches it in its next
/// prompt. `forced` is a conversation that ran out of turns.
async fn conclude(
    driver: &CompletionDriver<'_>,
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
        if forced { " (out of turns)" } else { "" }
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

fn seat(
    runtime: &Runtime,
    id: &str,
    brief: &str,
    contract: &str,
    run_id: &str,
    endpoint: &str,
) -> anyhow::Result<Agent> {
    // The hosted harness resolves an agent by its *definition* at turn time,
    // and a definition not in the registry is sanitised to "rejected by
    // policy" on the way out. Seed it.
    let agent_id = format!("{id}-conducted-{run_id}");
    let prompt = format!("{brief}\n\n{contract}");
    let registry_entry = AgentRegistryEntry {
        id: agent_id.clone(),
        name: id.to_owned(),
        description: "TinyHiveMind desk seat".into(),
        source: AgentRegistrySource::Custom,
        enabled: true,
        model: None,
        system_prompt: Some(prompt.clone()),
        tool_allowlist: dispatchers(),
        tool_denylist: Vec::new(),
        subagents: AgentSubagentPolicy::default(),
        tags: Vec::new(),
        metadata: serde_json::Value::Null,
    };
    Ok(runtime.agent(
        AgentSpec::new(agent_id)
            .config(move |config| config.agent_registry.entries.push(registry_entry))
            .system_prompt(prompt)
            .mcp(McpServer::http("episode", endpoint))
            .definition(
                AgentDefinitionSpec::new()
                    // OpenHuman reaches a remote MCP tool through three
                    // generic dispatchers. Allowlisting the destination
                    // blocks the only road to it; these three are the road.
                    .tools(ToolScopeSpec::Named(dispatchers()))
                    .max_iterations(16)
                    .temperature(0.0),
            ),
    )?)
}

fn dispatchers() -> Vec<String> {
    ["mcp_list_servers", "mcp_list_tools", "mcp_call_tool"]
        .into_iter()
        .map(str::to_owned)
        .collect()
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
