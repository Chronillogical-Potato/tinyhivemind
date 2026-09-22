//! One seat, run as a fresh raw session on every turn.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use openhuman_core::agent::prompts::SystemPromptBuilder;
use openhuman_core::agent::{OpenHumanSessionHost, TurnOverrides};
use openhuman_core::config::{AgentConfig, Config};
use openhuman_core::core::runtime::CoreContext;
use tinyhivemind_openhuman::BoundAgent;
use tinytools::Tool;
use tinytools_agent::dialect::NativeDialect;

use super::policy::{EpisodeGate, NoMemory};
use crate::conducted::runner::TURN_TIMEOUT;

/// The tool-loop ceiling for one turn: think, call, read the receipt, reply.
const MAX_TOOL_ITERATIONS: usize = 6;

/// Everything needed to build a seat's session, and no session.
///
/// Cloned per turn into the job that runs it, and bound into the hive as the
/// seat's own handle: the driver hands it back with a pending round and never
/// runs it, which is why nothing here is an agent.
#[derive(Clone)]
pub struct RawSeat {
    id: String,
    /// The standing prompt: the brief and the contract, whole.
    system_prompt: String,
    /// The resolved config every session is built from, carrying the route.
    config: Arc<Config>,
    /// The core context every session runs under: a library host, so the
    /// route in `config` is the credential and no sign-in is waited on.
    context: Arc<CoreContext>,
    /// The model id the config resolves `chat` to.
    model_name: String,
    /// The workspace a session is rooted in. Nothing is written there --
    /// `auto_save` is off -- but the builder wants a directory.
    workspace: PathBuf,
}

impl BoundAgent for RawSeat {
    fn runtime_id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for RawSeat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawSeat")
            .field("id", &self.id)
            .field("model_name", &self.model_name)
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

impl RawSeat {
    pub fn new(
        id: impl Into<String>,
        system_prompt: impl Into<String>,
        config: Arc<Config>,
        context: Arc<CoreContext>,
        model_name: impl Into<String>,
        workspace: PathBuf,
    ) -> Self {
        Self {
            id: id.into(),
            system_prompt: system_prompt.into(),
            config,
            context,
            model_name: model_name.into(),
            workspace,
        }
    }

    /// Build one session from this host's objects. Every setter here is one
    /// the embed facade cannot express: the belt, the memory, the policy and
    /// the prompt are objects, not configuration.
    fn session(&self, tools: Vec<Box<dyn Tool>>) -> anyhow::Result<OpenHumanSessionHost> {
        let names: Vec<String> = tools.iter().map(|tool| tool.name().to_owned()).collect();
        OpenHumanSessionHost::builder()
            // The same crate-native model source the production factory uses,
            // resolved from the config's `chat` role.
            .crate_native_provider("chat", Arc::clone(&self.config))
            .model_name(self.model_name.clone())
            .temperature(0.0)
            .tools(tools)
            .memory(Arc::new(NoMemory))
            .tool_dispatcher(Box::new(NativeDialect))
            .prompt_builder(SystemPromptBuilder::from_final_body(
                self.system_prompt.clone(),
            ))
            .tool_policy(Arc::new(EpisodeGate::new(names)))
            .config(AgentConfig {
                max_tool_iterations: MAX_TOOL_ITERATIONS,
                ..AgentConfig::default()
            })
            .workspace_dir(self.workspace.clone())
            .action_dir(self.workspace.clone())
            // The host's log is the only log. A session that also wrote
            // OpenHuman's transcript would be a second one.
            .auto_save(false)
            // The definition `prepare` registered for this seat: the hosted
            // turn resolves it by this name.
            .agent_definition_name(self.id.clone())
            .build()
    }

    /// Run one turn on a fresh session seeded with `history`, chronological
    /// `(role, content)` pairs, and drop it.
    ///
    /// # Errors
    ///
    /// The session failing to build or seed, the turn failing, or the turn
    /// running past its wall.
    pub async fn turn(
        &self,
        history: Vec<(String, String)>,
        message: &str,
        tools: Vec<Box<dyn Tool>>,
    ) -> anyhow::Result<String> {
        // The whole turn under the library context: the session is built,
        // and its model resolved, inside it.
        CoreContext::scope(Arc::clone(&self.context), async {
            let mut host = self.session(tools)?;
            host.seed_resume_from_messages(history, message)?;
            host.set_next_turn_overrides(TurnOverrides {
                suppress_transcript_autoload: true,
                ..TurnOverrides::default()
            });
            tokio::time::timeout(TURN_TIMEOUT, host.turn(message))
                .await
                .map_err(|_| anyhow::anyhow!("@{} timed out", self.id))?
        })
        .await
    }
}
