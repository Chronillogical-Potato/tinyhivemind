//! The embed runner: `openhuman-embed` agents, with the room's tools on
//! their belt.
//!
//! This is the runner the live episodes in `docs/experiments/` were recorded
//! with. A seat is an `AgentSpec` on the runtime, and there are two roads to
//! the same tools: [`EmbedRunner::seat`] hands them to the spec itself
//! through `AgentSpec::tools`, which is the default, and
//! [`EmbedRunner::seat_over_mcp`] reaches them through `OpenHuman`'s three
//! MCP dispatchers against `tinyhivemind-mcp`'s server. The second was the
//! only road before a spec could carry a belt of its own.
//!
//! A seat keeps one session for the whole episode and **seeds** it every turn
//! from the host's journal, as [`hosted`](crate::hosted) does: the rows it may
//! read, its persona at their head, and the turn's new rows in the brief.
//!
//! It used to resume that session instead, on the reasoning that `OpenHuman`'s
//! own continuity is what an `AgentSpec` seat has and a host's journal is a
//! second copy of it. What that cost: a resumed turn binds the session's
//! transcript the first time it commits, and the next turn is refused unless
//! its target is the *same binding* -- compared by pointer, against a locator
//! the runtime rebuilds per call. So every seat failed on its second turn with
//! `cannot change a transcript target after it is bound or committed`, and an
//! episode stalled the moment a seat spoke twice: a seat that asked and was
//! woken by the answer, a nudged seat, a seat refused a completion. Seeding
//! replaces resume rather than adding to it, so nothing binds and nothing is
//! compared.
//!
//! The runtime is the host's: it chooses the provider, the access tier and
//! the workspace. One thing it must say is [`EmbedRunner::services`]: without
//! MCP boot the subsystem never dials the episode's endpoint, and the seats
//! are never offered a tool at all, which reads exactly like a model
//! declining to call one.

#[cfg(test)]
mod test;

use std::collections::BTreeMap;
use std::sync::Arc;

use openhuman_core::agent::registry::types::{
    AgentRegistryEntry, AgentRegistrySource, AgentSubagentPolicy,
};
use openhuman_embed::{
    Agent, AgentDefinitionSpec, AgentSpec, HostTurnTools, McpServer, Runtime, ServiceSet,
    ToolScopeSpec,
};
use tinyhivemind_driver::{AgentBinding, BoundAgent};
use tinyhivemind_mcp::{EpisodeTools, Server, serve};

use crate::episode::Journal;
use crate::raw::tools::belt_with_prefix;
use crate::runner::{Lane, SeatRunner, TURN_TIMEOUT, TurnJob, TurnResult, unseated};
use crate::{Error, Result};
use tinyhivemind::{Conversation, Sequence};

/// An `openhuman-embed` agent as the handle the driver binds.
///
/// A newtype because the trait and the agent are both foreign to this crate;
/// the driver stores it and hands it back, and the runner runs the agent.
#[derive(Clone, Debug)]
pub struct EmbedSeat(pub Agent);

impl BoundAgent for EmbedSeat {
    fn runtime_id(&self) -> &str {
        self.0.id()
    }
}

/// Seats as `openhuman-embed` agents, with the room's tools on their belt.
pub struct EmbedRunner {
    journal: Arc<dyn Journal>,
    tools: Arc<EpisodeTools>,
    agents: BTreeMap<String, Agent>,
    /// Each seat's standing prompt, kept from when it was seated.
    ///
    /// A seeded turn after a seat's first renders no system prompt of its
    /// own -- the seed clears the conversation the spec composed one into --
    /// so it goes back at the head of every turn's history, as it does for a
    /// hosted seat.
    personas: BTreeMap<String, String>,
    desk: Conversation,
    window: usize,
    run_id: String,
    /// Held so the endpoint outlives every turn; dropping it stops the
    /// server. `None` for a native belt, which opens no socket.
    _server: Option<Server>,
}

impl EmbedRunner {
    /// What an embed seat needs of its runtime's services: MCP boot, and
    /// nothing else.
    ///
    /// Only [`seat_over_mcp`](Self::seat_over_mcp) needs it. A native belt is
    /// handed to the spec directly and dials nothing, so a runtime built for
    /// [`seat`](Self::seat) alone may say [`ServiceSet::none`]. A host that
    /// builds one runtime for both uses this.
    #[must_use]
    pub fn services() -> ServiceSet {
        let mut services = ServiceSet::none();
        services.mcp_boot = true;
        services
    }

    /// Serve the tools and seat every brief as an agent on `runtime`.
    ///
    /// `journal` is the host's, read as each seat to seed its turn; `desk`
    /// and `desk_name` name the desk those turns run on or in a thread of, as
    /// that journal knows it; `window` bounds how many rows a turn is seeded
    /// with, and `tinyhivemind::SESSION_WINDOW` is what the rest of the crate
    /// reads with. `run_id` keeps agent ids unique across episodes on one
    /// runtime, which refuses a second agent of the same id.
    ///
    /// # Errors
    ///
    /// An agent failing to instantiate.
    #[allow(clippy::too_many_arguments)]
    pub fn seat(
        journal: Arc<dyn Journal>,
        runtime: &Runtime,
        tools: Arc<EpisodeTools>,
        briefs: &BTreeMap<String, String>,
        contract: &str,
        desk: &str,
        desk_name: &str,
        window: usize,
        run_id: &str,
    ) -> Result<Self> {
        Self::build(
            journal, runtime, tools, briefs, contract, desk, desk_name, window, run_id, None,
        )
    }

    /// The same seats, reaching the same tools over MCP instead.
    ///
    /// One endpoint per seat on `tinyhivemind-mcp`'s server, dialled through
    /// `OpenHuman`'s three MCP dispatchers. This was the only road before
    /// `AgentSpec` could carry a belt of its own, and it stays because it is
    /// the one thing in this repository that exercises the socket ADR 0022
    /// opens, and because what a tool call costs over a wire is worth being
    /// able to measure against what it costs in-process.
    ///
    /// The runtime must be built with [`services`](Self::services); without
    /// MCP boot the subsystem never dials the endpoint and the seats are
    /// never offered a tool at all, which reads exactly like a model
    /// declining to call one.
    ///
    /// # Errors
    ///
    /// The server failing to bind, or an agent failing to instantiate.
    #[allow(clippy::too_many_arguments)]
    pub async fn seat_over_mcp(
        journal: Arc<dyn Journal>,
        runtime: &Runtime,
        tools: Arc<EpisodeTools>,
        briefs: &BTreeMap<String, String>,
        contract: &str,
        desk: &str,
        desk_name: &str,
        window: usize,
        run_id: &str,
    ) -> Result<Self> {
        // One endpoint per seat: identity is the URL dialled, never a field
        // filled in.
        let server = serve(Arc::clone(&tools)).await?;
        Self::build(
            journal,
            runtime,
            tools,
            briefs,
            contract,
            desk,
            desk_name,
            window,
            run_id,
            Some(server),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        journal: Arc<dyn Journal>,
        runtime: &Runtime,
        tools: Arc<EpisodeTools>,
        briefs: &BTreeMap<String, String>,
        contract: &str,
        desk: &str,
        desk_name: &str,
        window: usize,
        run_id: &str,
        server: Option<Server>,
    ) -> Result<Self> {
        let mut agents = BTreeMap::new();
        let mut personas = BTreeMap::new();
        for (id, brief) in briefs {
            let prompt = persona(brief, contract);
            let agent = match &server {
                Some(server) => over_mcp(runtime, id, &prompt, run_id, &server.endpoint(id))?,
                None => natively(runtime, id, &prompt, run_id, &tools)?,
            };
            agents.insert(id.clone(), agent);
            personas.insert(id.clone(), prompt);
        }
        Ok(Self {
            journal,
            tools,
            agents,
            personas,
            desk: Conversation {
                desk_id: desk.to_owned(),
                desk_name: desk_name.to_owned(),
                thread_root: None,
            },
            window,
            run_id: run_id.to_owned(),
            _server: server,
        })
    }
}

impl std::fmt::Debug for EmbedRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedRunner")
            .field("seats", &self.agents.keys().collect::<Vec<_>>())
            .field("run_id", &self.run_id)
            .finish_non_exhaustive()
    }
}

impl SeatRunner for EmbedRunner {
    fn tools(&self) -> &Arc<EpisodeTools> {
        &self.tools
    }

    type Bound = EmbedSeat;

    fn bindings(&self) -> Vec<AgentBinding<EmbedSeat>> {
        self.agents
            .iter()
            .map(|(id, agent)| AgentBinding::new(id.clone(), EmbedSeat(agent.clone())))
            .collect()
    }

    /// One session per seat for the whole episode, seeded from the host's
    /// journal every turn: what the seat may read up to its watermark, its
    /// persona at the head, and the turn's new rows in `prompt`.
    fn turn(&self, seat: String, lane: Lane, since: Option<Sequence>, prompt: String) -> TurnJob {
        let Some(agent) = self.agents.get(&seat).cloned() else {
            return unseated(seat, lane);
        };
        let session = format!("episode-{}:{seat}", self.run_id);
        let journal = Arc::clone(&self.journal);
        let persona = self.personas.get(&seat).cloned();
        let window = self.window;
        let conversation = Conversation {
            thread_root: match lane {
                Lane::Desk => None,
                Lane::Thread(root) => Some(root),
            },
            ..self.desk.clone()
        };
        Box::pin(async move {
            let seeded =
                crate::seed::history(journal.log(), conversation, &seat, since, window, &|id| {
                    journal.display_name(id)
                })
                .await;
            let result = match seeded {
                Err(error) => TurnResult::Failed(error.to_string()),
                Ok(history) => {
                    let history = crate::seed::with_persona(history, persona);
                    match tokio::time::timeout(
                        TURN_TIMEOUT,
                        agent.turn(prompt).session(&session).seed(history).send(),
                    )
                    .await
                    {
                        Ok(Ok(outcome)) => TurnResult::Replied(outcome.reply),
                        Ok(Err(error)) => TurnResult::Failed(error.to_string()),
                        Err(_) => {
                            TurnResult::Failed(Error::TimedOut { seat: seat.clone() }.to_string())
                        }
                    }
                }
            };
            (seat, lane, result)
        })
    }
}

/// A seat's standing prompt: its brief, then the contract every seat shares.
fn persona(brief: &str, contract: &str) -> String {
    format!("{brief}\n\n{contract}")
}

/// One seat whose belt is the episode's, handed to the spec directly.
///
/// `AgentSpec::tools` takes a factory run per turn, which is what the belt
/// wants: `EpisodeTools` is a record a host drains between turns, and a belt
/// built once would close over a stale view of it. The tools carry their own
/// names, so the definition's scope names them too -- a wildcard scope
/// projects to no declared names and the host fails closed.
fn natively(
    runtime: &Runtime,
    id: &str,
    prompt: &str,
    run_id: &str,
    tools: &Arc<EpisodeTools>,
) -> Result<Agent> {
    let agent_id = format!("{id}-{run_id}");
    let prompt = prompt.to_owned();
    let names: Vec<String> = tools.specs().map(|spec| spec.name.to_owned()).collect();
    let registry_entry = registry_entry(&agent_id, id, &prompt, names.clone());
    let belt_tools = Arc::clone(tools);
    let seat = id.to_owned();
    Ok(runtime.agent(
        AgentSpec::new(agent_id)
            .config(move |config| config.agent_registry.entries.push(registry_entry))
            .system_prompt(prompt)
            .tools(move |_turn| HostTurnTools::advertised(belt_with_prefix(&seat, &belt_tools, "")))
            .definition(
                AgentDefinitionSpec::new()
                    .tools(ToolScopeSpec::Named(names))
                    .max_iterations(16)
                    .temperature(0.0),
            ),
    )?)
}

/// One seat that reaches the same tools over MCP: three dispatchers and an
/// endpoint of its own.
fn over_mcp(
    runtime: &Runtime,
    id: &str,
    prompt: &str,
    run_id: &str,
    endpoint: &str,
) -> Result<Agent> {
    let agent_id = format!("{id}-{run_id}");
    let prompt = prompt.to_owned();
    let registry_entry = registry_entry(&agent_id, id, &prompt, dispatchers());
    Ok(runtime.agent(
        AgentSpec::new(agent_id)
            .config(move |config| config.agent_registry.entries.push(registry_entry))
            .system_prompt(prompt)
            .mcp(McpServer::http("episode", endpoint))
            .definition(
                AgentDefinitionSpec::new()
                    // `OpenHuman` does not surface a remote MCP tool as a tool of
                    // its own. It registers three generic dispatchers and the
                    // agent reaches a server *through* them; these three are
                    // the road, and every other built-in stays out.
                    .tools(ToolScopeSpec::Named(dispatchers()))
                    .max_iterations(16)
                    .temperature(0.0),
            ),
    )?)
}

/// The registry entry a seat is resolved by, with the belt it may call.
fn registry_entry(
    agent_id: &str,
    seat: &str,
    prompt: &str,
    allowlist: Vec<String>,
) -> AgentRegistryEntry {
    AgentRegistryEntry {
        id: agent_id.to_owned(),
        name: seat.to_owned(),
        description: "TinyHiveMind desk seat".into(),
        source: AgentRegistrySource::Custom,
        enabled: true,
        model: None,
        system_prompt: Some(prompt.to_owned()),
        tool_allowlist: allowlist,
        tool_denylist: Vec::new(),
        subagents: AgentSubagentPolicy::default(),
        tags: Vec::new(),
        metadata: serde_json::Value::Null,
    }
}

fn dispatchers() -> Vec<String> {
    ["mcp_list_servers", "mcp_list_tools", "mcp_call_tool"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}
