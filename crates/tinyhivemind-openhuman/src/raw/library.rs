//! The core booted as a library host, and the sessions built on it.
//!
//! A raw session runs inside the core the way a library embedder's does. The
//! core reads its ambient context to decide whose product policy applies;
//! with none it is the desktop's, and inference waits on the operator signing
//! in. `HostKind::Library` says the caller owns the provider and its
//! credential -- this config's route -- and is what the embed runtime says of
//! itself when it boots. Nothing else is asked of the core: no domain, no
//! service, no store.
//!
//! [`RawRunner`](super::RawRunner) seats on one of these, and so can any host
//! that has no core of its own to build sessions on: [`LibraryHost::session`]
//! builds one from the objects a spec cannot carry, and
//! [`LibraryHost::scope`] runs a turn under the context.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use openhuman_core::agent::OpenHumanSessionHost;
use openhuman_core::agent::prompts::SystemPromptBuilder;
use openhuman_core::agent::tool_policy::ToolPolicy;
use openhuman_core::config::schema::ephemeral_route::{self, EphemeralRoute};
use openhuman_core::config::{AgentConfig, Config};
use openhuman_core::core::runtime::{CoreContext, DomainSet, TokenSource};
use openhuman_core::core::types::HostKind;
use openhuman_core::tools::toolpacks::ToolGroups;
use tinytools::Tool;
use tinytools_agent::dialect::NativeDialect;

use super::Route;
use super::policy::NoMemory;
use crate::{Error, Result};

/// The tool-loop ceiling for one turn: think, call, read the receipt, reply.
const MAX_TOOL_ITERATIONS: usize = 6;

/// A core booted as a library host over one resolved route.
#[derive(Clone)]
pub struct LibraryHost {
    /// The resolved config every session is built from, carrying the route.
    config: Arc<Config>,
    /// The context every turn runs under.
    context: Arc<CoreContext>,
    /// What the config resolves `chat` to.
    model: String,
    /// Where a session is rooted. Nothing is written there -- `auto_save` is
    /// off -- but the builder wants a directory.
    workspace: PathBuf,
}

impl std::fmt::Debug for LibraryHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibraryHost")
            .field("model", &self.model)
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

impl LibraryHost {
    /// Boot the core as a library host over `base`, with the workspace, the
    /// backend and the route written in, and resolve the `chat` role once so
    /// a route the factory cannot resolve fails here rather than at the first
    /// turn.
    ///
    /// # Errors
    ///
    /// An incomplete route, the core refusing to boot, or the route failing
    /// to resolve.
    pub async fn boot(
        base: &Config,
        backend_url: &str,
        route: &Route,
        workspace: &Path,
    ) -> Result<Self> {
        let mut config = base.clone();
        config.workspace_dir = workspace.to_path_buf();
        config.action_dir = workspace.to_path_buf();
        config.api_url = Some(backend_url.to_owned());
        config.default_model = Some(route.model.clone());
        let ephemeral =
            EphemeralRoute::from_params(Some(route.endpoint.clone()), Some(route.api_key.clone()))
                .ok_or(Error::IncompleteRoute)?;
        ephemeral_route::apply(&mut config, ephemeral);
        let config = Arc::new(config);
        let (context, _, _) = Box::pin(CoreContext::init_with_config(
            HostKind::Library,
            &TokenSource::Fixed(Arc::new(format!("tinyhivemind-raw-{}", std::process::id()))),
            DomainSet::none(),
            ToolGroups::default(),
            Some((*config).clone()),
            None,
        ))
        .await?;
        let (_, model) = CoreContext::scope(Arc::clone(&context), async {
            openhuman_core::inference::provider::create_chat_model_with_model_id(
                "chat", &config, 0.0,
            )
        })
        .await?;
        Ok(Self {
            config,
            context,
            model,
            workspace: workspace.to_path_buf(),
        })
    }

    /// The model id the config resolved `chat` to.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Build one session for `seat` from objects: the belt, the policy and
    /// the prompt, over a memory that keeps nothing. `seat` is also the
    /// definition name the hosted turn resolves, so it must be registered.
    ///
    /// # Errors
    ///
    /// The builder refusing the session.
    pub fn session(
        &self,
        seat: &str,
        system_prompt: &str,
        tools: Vec<Box<dyn Tool>>,
        policy: Arc<dyn ToolPolicy>,
    ) -> Result<OpenHumanSessionHost> {
        OpenHumanSessionHost::builder()
            // The same crate-native model source the production factory uses,
            // resolved from the config's `chat` role.
            .crate_native_provider("chat", Arc::clone(&self.config))
            .model_name(self.model.clone())
            .temperature(0.0)
            .tools(tools)
            .memory(Arc::new(NoMemory))
            .tool_dispatcher(Box::new(NativeDialect))
            .prompt_builder(SystemPromptBuilder::from_final_body(
                system_prompt.to_owned(),
            ))
            .tool_policy(policy)
            .config(AgentConfig {
                max_tool_iterations: MAX_TOOL_ITERATIONS,
                ..AgentConfig::default()
            })
            .workspace_dir(self.workspace.clone())
            .action_dir(self.workspace.clone())
            // The host's log is the only log. A session that also wrote
            // `OpenHuman`'s transcript would be a second one.
            .auto_save(false)
            .agent_definition_name(seat.to_owned())
            .build()
            .map_err(Error::Harness)
    }

    /// Run `turn` under this host's context: a session is built, and its
    /// model resolved, inside it.
    pub async fn scope<F: Future>(&self, turn: F) -> F::Output {
        Box::pin(CoreContext::scope(Arc::clone(&self.context), turn)).await
    }
}
