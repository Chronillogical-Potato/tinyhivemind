//! The embed runner: `openhuman-embed` agents, tools over MCP.
//!
//! This is the runner the live episodes in `docs/experiments/` were recorded
//! with. A seat is an `AgentSpec` on the runtime, holding a stable session
//! across the episode, and reaches the room's tools through OpenHuman's three
//! MCP dispatchers against `tinyhivemind-mcp`'s server -- the only road
//! `AgentSpec` offers for a tool the runtime did not ship.

use std::collections::BTreeMap;
use std::sync::Arc;

use openhuman_core::agent::registry::types::{
    AgentRegistryEntry, AgentRegistrySource, AgentSubagentPolicy,
};
use openhuman_embed::{Agent, AgentDefinitionSpec, AgentSpec, McpServer, Runtime, ToolScopeSpec};
use tinyhivemind_mcp::{EpisodeTools, Server, serve};
use tinyhivemind_openhuman::AgentBinding;

use super::runner::{Lane, SeatRunner, TURN_TIMEOUT, TurnJob};

/// Seats as `openhuman-embed` agents, with the room's tools served over MCP.
pub struct EmbedRunner {
    tools: Arc<EpisodeTools>,
    agents: BTreeMap<String, Agent>,
    run_id: String,
    /// Held so the endpoint outlives every turn; dropping it stops the server.
    _server: Server,
}

impl EmbedRunner {
    /// Serve the tools and seat every brief as an agent on `runtime`.
    ///
    /// # Errors
    ///
    /// The server failing to bind, or an agent failing to instantiate.
    pub async fn seat(
        runtime: &Runtime,
        tools: Arc<EpisodeTools>,
        briefs: &BTreeMap<String, String>,
        contract: &str,
        run_id: &str,
    ) -> anyhow::Result<Self> {
        // One endpoint per seat: identity is the URL dialled, never a field
        // filled in.
        let server = serve(Arc::clone(&tools)).await?;
        let mut agents = BTreeMap::new();
        for (id, brief) in briefs {
            let agent = seat(runtime, id, brief, contract, run_id, &server.endpoint(id))?;
            eprintln!(
                "[seat] {id}: {} mcp server(s) registered",
                agent.config().mcp_client.servers.len()
            );
            agents.insert(id.clone(), agent);
        }
        Ok(Self {
            tools,
            agents,
            run_id: run_id.to_owned(),
            _server: server,
        })
    }
}

impl SeatRunner for EmbedRunner {
    fn tools(&self) -> &Arc<EpisodeTools> {
        &self.tools
    }

    type Bound = Agent;

    fn bindings(&self) -> Vec<AgentBinding<Agent>> {
        self.agents
            .iter()
            .map(|(id, agent)| AgentBinding::new(id.clone(), agent.clone()))
            .collect()
    }

    /// One session per seat for the whole episode, so OpenHuman appends to the
    /// context the agent already holds rather than rebuilding one.
    fn turn(&self, seat: String, lane: Lane, prompt: String) -> TurnJob {
        let agent = self.agents[&seat].clone();
        let session = format!("conducted-{}:{seat}", self.run_id);
        Box::pin(async move {
            let result = match tokio::time::timeout(
                TURN_TIMEOUT,
                agent.turn(prompt).session(&session).send(),
            )
            .await
            {
                Ok(Ok(outcome)) => Some(Ok(outcome.reply)),
                Ok(Err(error)) => Some(Err(error.to_string())),
                Err(_) => None,
            };
            (seat, lane, result)
        })
    }
}

fn seat(
    runtime: &Runtime,
    id: &str,
    brief: &str,
    contract: &str,
    run_id: &str,
    endpoint: &str,
) -> anyhow::Result<Agent> {
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
                    // OpenHuman does not surface a remote MCP tool as a tool of
                    // its own. It registers three generic dispatchers and the
                    // agent reaches a server *through* them; these three are
                    // the road, and every other built-in stays out.
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
