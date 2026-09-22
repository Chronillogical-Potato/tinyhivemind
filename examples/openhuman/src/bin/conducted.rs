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
    AgentBinding, BroadcastRouting, CommittedUtterance, CompletionDriver, DriverState, Error,
    HiveGraph, HostAction, OpenHumanHive,
};
use tinyhivemind_typesafe::JevRouter;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// The protocol, stated where an agent actually keeps it: the standing prompt.
const PROTOCOL: &str = "\
You are one seat on a desk. You have no codebase, shell or filesystem -- only
the desk's messages and your own judgement. Never ask for permission and never
wait to be told to continue; nobody will answer.

Your work is recorded by calling a tool on the MCP server named `episode`.
Prose alone changes nothing: if you end a turn without calling one, nothing you
said is recorded and the desk does not move.

Use `mcp_call_tool` with `server: \"episode\"`. Every call carries
`\"chat\": \"engineering\"` and `\"parent\": null` -- exactly those -- beside its
own arguments:

  tool \"post\", arguments {\"message\": \"...\"}
      -- say one thing to the whole desk: a finding, or your answer to a
         question a peer asked you.

  tool \"broadcast\", arguments {\"message\": \"<the work you found>\"}
      -- hand work you found to whichever seat it belongs to. Routing chooses;
         you do not name anyone, and it does not finish your own work.

  tool \"ask\", arguments {\"to\": \"<seat id>\", \"message\": \"<what you need>\"}
      -- ask one named seat something. It is answered on its own time, not
         while you wait: ask everything you need, then end your turn, and the
         answers reach you on a later one. You cannot finish until every
         answer has arrived. It is a question, not a handoff: the work stays
         yours. The tool's own schema lists the seats you may name.

  tool \"complete_episode\", arguments {\"message\": \"<what you concluded>\"}
      -- call once, when you have finished what was asked of you.

Call broadcast and then complete_episode when the work you found is not yours
at all. Keep your reply brief -- the tool message is what the desk reads.";

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
const TURN_WALL: u64 = 40;

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
    /// A private row reaches one seat; `None` is the whole desk.
    only_for: Option<String>,
}

#[derive(Default)]
struct Journal {
    rows: Mutex<Vec<Row>>,
}

impl Journal {
    fn append(&self, author: &str, body: &str, only_for: Option<&str>) -> Sequence {
        let mut rows = self.rows.lock().expect("journal is not poisoned");
        let sequence = Sequence(rows.last().map_or(0, |row| row.sequence.0) + 1);
        rows.push(Row {
            sequence,
            author: author.to_owned(),
            body: body.to_owned(),
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

    /// What `seat` may read above `after`: desk rows and its own private ones.
    fn visible_since(&self, seat: &str, after: Sequence) -> Vec<String> {
        self.rows
            .lock()
            .expect("journal is not poisoned")
            .iter()
            .filter(|row| row.sequence > after)
            .filter(|row| row.only_for.as_deref().is_none_or(|only| only == seat))
            .map(|row| format!("@{}: {}", row.author, row.body))
            .collect()
    }

    fn all(&self) -> Vec<Row> {
        self.rows.lock().expect("journal is not poisoned").clone()
    }
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
    let mut agents: BTreeMap<String, Agent> = BTreeMap::new();
    for (id, _, _) in SEATS {
        let agent = seat(&runtime, id, &briefs[id], &run_id, &server.endpoint(id))?;
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
    journal.append("operator", TASK, None);

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
    let mut turns = 0_u64;
    let mut waves = 0_u64;
    let mut discharged = 0_u64;
    let settled = 'episode: loop {
        if state.quiescent() {
            break Ok(());
        }
        let round = driver.pending_round(&state)?;
        let seats: Vec<String> = round
            .agents()
            .iter()
            .map(|pending| pending.hive_agent_id.to_owned())
            .collect();
        if seats.is_empty() {
            break Err(anyhow::anyhow!(
                "episode stalled with {:?} holding open work",
                state.stalled()
            ));
        }
        waves += 1;

        // Prepare every seat in the round, then run them together.
        let mut jobs = Vec::new();
        for seat_id in &seats {
            let through = state
                .seen()
                .delivered_through
                .get(seat_id)
                .copied()
                .unwrap_or(Sequence(0));
            let rows = journal.visible_since(seat_id, through);
            let latest = journal.latest();
            tools.window(seat_id, rows.clone());
            tools.register(
                seat_id,
                Dispatch {
                    chat: DESK_ID.into(),
                    parent: None,
                },
            );
            state.delivered(seat_id, latest);
            state.turn_started(seat_id);
            let prompt = turn_prompt(&state, seat_id, &rows);
            let session = format!("conducted-{run_id}:{seat_id}");
            let agent = agents[seat_id].clone();
            let id = seat_id.clone();
            jobs.push(async move {
                let outcome =
                    tokio::time::timeout(TURN_TIMEOUT, agent.turn(prompt).session(&session).send())
                        .await;
                (id, outcome)
            });
        }
        let outcomes = futures::future::join_all(jobs).await;

        for (seat_id, outcome) in outcomes {
            tools.clear(&seat_id);
            turns += 1;
            match outcome {
                Ok(Ok(reply)) => eprintln!(
                    "[turn] @{seat_id} replied ({} chars)",
                    reply.reply.chars().count()
                ),
                Ok(Err(error)) => eprintln!("[turn] @{seat_id} failed: {error}"),
                Err(_) => eprintln!("[turn] @{seat_id} timed out"),
            }
            let events = tools.drain(&seat_id);
            if events.is_empty() {
                eprintln!("[no tool call] @{seat_id}");
            }
            for event in events {
                let ToolCall::Speak(utterance) = event.call else {
                    continue;
                };
                // An ask is private to the seat it asks; everything else is the desk's.
                let only_for = utterance.asks().map(str::to_owned);
                let sequence = journal.append(&seat_id, &describe(&utterance), only_for.as_deref());
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
                        for action in &transition.actions {
                            match action {
                                HostAction::RunAgents { agent_ids, .. } => {
                                    println!("[broadcast] @{seat_id} -> {}", agent_ids.join(", "));
                                }
                                HostAction::DeliverDm { .. } => {}
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
                                        Some(agent_id),
                                    );
                                }
                            }
                        }
                        state = transition.state;
                    }
                    Err(Error::AwaitingReply { waiting_on, .. }) => {
                        eprintln!(
                            "[refused] @{seat_id} may not complete: waiting on {waiting_on:?}"
                        );
                        journal.append(
                            "desk",
                            &format!(
                                "your completion was refused: you are still waiting on {}. \
                                 Their answer arrives on a later turn; complete after it does.",
                                waiting_on.join(", ")
                            ),
                            Some(&seat_id),
                        );
                    }
                    Err(Error::BudgetSpent { .. }) => {
                        eprintln!(
                            "[refused] @{seat_id} has spent its broadcast budget; it keeps the work"
                        );
                        discharged += 1;
                        let sequence =
                            journal.append(&seat_id, "budget spent; keeping the work", None);
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
        if turns >= TURN_WALL {
            break Err(anyhow::anyhow!("turn wall of {TURN_WALL} reached"));
        }
    };

    println!(
        "turns {turns} | routes {} | waves {waves} | discharged {discharged} | settled {}",
        router.calls.load(Ordering::SeqCst),
        state.episode().settled()
    );
    for row in journal.all() {
        let scope = row
            .only_for
            .as_deref()
            .map_or(String::new(), |only| format!(" (to @{only})"));
        println!(
            "  {:>3}  @{}{scope}: {}",
            row.sequence.0, row.author, row.body
        );
    }
    drop(server);
    settled
}

/// What one turn is shown: only the rows above its watermark, its assignment,
/// and the one thing the tools need it to say on every call.
fn turn_prompt(state: &DriverState, seat: &str, rows: &[String]) -> String {
    let rows = if rows.is_empty() {
        "(nothing new)".to_owned()
    } else {
        rows.join("\n")
    };
    let assignment = state
        .episode()
        .participants
        .iter()
        .find(|participant| participant.agent_id == seat)
        .and_then(|participant| participant.open())
        .map_or_else(
            || {
                "You hold no open assignment. A peer asked you something above: answer it \
                 with `post`, from what you privately know, and end your turn."
                    .to_owned()
            },
            |record| {
                format!(
                    "Your assignment was made at sequence {}.",
                    record.assigned_at.0
                )
            },
        );
    format!(
        "## New desk messages\n{rows}\n\n{assignment}\n\nEvery tool call must carry \
         \"chat\": \"{DESK_ID}\" and \"parent\": null."
    )
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
    run_id: &str,
    endpoint: &str,
) -> anyhow::Result<Agent> {
    // The hosted harness resolves an agent by its *definition* at turn time,
    // and a definition not in the registry is sanitised to "rejected by
    // policy" on the way out. Seed it.
    let agent_id = format!("{id}-conducted-{run_id}");
    let prompt = format!("{brief}\n\n{PROTOCOL}");
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
