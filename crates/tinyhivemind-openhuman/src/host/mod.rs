//! Registration and execution of host-owned agent handles.
mod activation;
mod runner;
mod types;
use crate::{Error, Result};
pub(crate) use activation::Activation;
use openhuman_embed::{Agent, HostTools, HostTurnTools};
use runner::SuppliedRunner;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
use tinyhivemind_hives::{AgentRegistration, Coordinator};
pub use types::*;

pub(crate) struct Inner {
    pub coordinator: Coordinator,
    runtime_id: String,
    agents: Mutex<BTreeMap<String, Entry>>,
    management: Option<Management>,
    hooks: Arc<dyn TurnHooks>,
}
struct Entry {
    agent: Agent,
    source: HostTools,
    runner: Arc<SuppliedRunner>,
    activation: Arc<Activation>,
}
/// Shares one coordinator and permanent attachment per supplied agent.
///
/// The host configures agents on one runtime before registration. Each handle
/// keeps one continuing conversation across hives. Keep this host alive while
/// its tools are used: attachment services hold weak references to it.
#[derive(Clone)]
pub struct OpenHumanHost {
    pub(crate) inner: Arc<Inner>,
}
impl std::fmt::Debug for OpenHumanHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenHumanHost")
            .field("runtime_id", &self.inner.runtime_id)
            .finish_non_exhaustive()
    }
}
impl OpenHumanHost {
    /// Bind an adapter to the same runtime as its coordinator.
    /// # Errors
    /// Reject mismatched runtime identities.
    pub fn new(runtime_id: String, coordinator: Coordinator) -> Result<Self> {
        if runtime_id != coordinator.runtime_id() {
            return Err(Error::RuntimeMismatch);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                coordinator,
                runtime_id,
                agents: Mutex::new(BTreeMap::new()),
                management: None,
                hooks: Arc::new(DefaultHooks),
            }),
        })
    }
    /// Configure management before sharing or registering agents.
    /// # Errors
    /// Refuse changed settings after registration or sharing the host.
    pub fn with_management(
        mut self,
        factory: Arc<dyn AgentFactory>,
        authorizer: Arc<dyn ManagementAuthorizer>,
    ) -> Result<Self> {
        let inner = self.configurable_inner()?;
        inner.management = Some(Management {
            factory,
            authorizer,
        });
        Ok(self)
    }
    /// Configure per-turn host hooks before sharing or registration.
    /// # Errors
    /// Refuse changed settings once agents or another host clone exist.
    pub fn with_hooks(mut self, hooks: Arc<dyn TurnHooks>) -> Result<Self> {
        let inner = self.configurable_inner()?;
        inner.hooks = hooks;
        Ok(self)
    }
    fn configurable_inner(&mut self) -> Result<&mut Inner> {
        if !self
            .inner
            .agents
            .lock()
            .map_err(|_| Error::Poisoned)?
            .is_empty()
        {
            return Err(Error::ManagementAlreadyStarted);
        }
        Arc::get_mut(&mut self.inner).ok_or(Error::ManagementAlreadyStarted)
    }
    /// Register an existing handle; repeated clones use the identical factory.
    ///
    /// Retains the supplied handle and installs nine permanent tools, or thirteen
    /// when management was configured. Configuration remains owned by the agent.
    /// An existing host conversation should use [`Self::register_agent_in_session`].
    /// # Errors
    /// Reject another runtime, conflicting handles, tool collisions or storage failures.
    pub fn register_agent(&self, agent: Agent) -> Result<()> {
        self.register(agent, None)
    }
    fn register(&self, agent: Agent, session_id: Option<&str>) -> Result<()> {
        if agent.runtime_id() != self.inner.runtime_id {
            return Err(Error::RuntimeMismatch);
        }
        let mut entries = self.inner.agents.lock().map_err(|_| Error::Poisoned)?;
        let id = agent.id().to_owned();
        if let Some(entry) = entries.get(&id) {
            if !entry.agent.same_agent(&agent) {
                return Err(Error::AgentConflict(id));
            }
            agent.attach_tools("hivemind", entry.source.clone())?;
            self.register_runner(
                AgentRegistration {
                    agent_id: id,
                    runtime_id: self.inner.runtime_id.clone(),
                    runner: entry.runner.clone(),
                },
                session_id,
            )?;
            entry.activation.activate();
            return Ok(());
        }
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        let actor = id.clone();
        let managed = self.inner.management.is_some();
        let activation = Arc::new(Activation::default());
        let attached_activation = activation.clone();
        let source: HostTools = Arc::new(move |_| {
            let tools = crate::tools::belt(&actor, &weak, &attached_activation, managed);
            let permanent = tools.iter().map(|tool| tool.name().to_owned()).collect();
            HostTurnTools {
                permanent,
                ..HostTurnTools::advertised(tools)
            }
        });
        agent.attach_tools("hivemind", source.clone())?;
        let runner = Arc::new(SuppliedRunner {
            agent: agent.clone(),
            hooks: self.inner.hooks.clone(),
            activation: activation.clone(),
        });
        // Keep the exact source for retry even if durable registration fails.
        entries.insert(
            id.clone(),
            Entry {
                agent,
                source,
                runner: runner.clone(),
                activation: activation.clone(),
            },
        );
        self.register_runner(
            AgentRegistration {
                agent_id: id,
                runtime_id: self.inner.runtime_id.clone(),
                runner,
            },
            session_id,
        )?;
        activation.activate();
        Ok(())
    }
    /// Bind a supplied agent to an already running host conversation.
    ///
    /// Commits the validated session binding before publishing its runner, so
    /// concurrent claims continue this conversation from their first turn.
    /// Failed durable registration keeps the identical attachment for retry;
    /// its tools remain inactive until registration succeeds.
    /// # Errors
    /// Registration failures or conflicting continuing-session bindings.
    pub fn register_agent_in_session(&self, agent: Agent, session_id: &str) -> Result<()> {
        self.register(agent, Some(session_id))
    }
    fn register_runner(
        &self,
        registration: AgentRegistration,
        session_id: Option<&str>,
    ) -> Result<()> {
        match session_id {
            Some(session) => self
                .inner
                .coordinator
                .register_agent_in_session(registration, session)?,
            None => self.inner.coordinator.register_agent(registration)?,
        }
        Ok(())
    }

    /// Access host APIs for hive creation, membership, sends and scheduling.
    #[must_use]
    pub fn coordinator(&self) -> &Coordinator {
        &self.inner.coordinator
    }
    /// Execute an authorized management request.
    /// # Errors
    /// Disabled management, host denial, factory, runtime or coordinator errors.
    pub async fn manage(
        &self,
        actor: &str,
        request: ManagementRequest,
    ) -> Result<serde_json::Value> {
        let management = self
            .inner
            .management
            .as_ref()
            .ok_or(Error::ManagementDisabled)?;
        management.authorizer.authorize(actor, &request)?;
        match request {
            ManagementRequest::CreateHive(hive) => {
                self.coordinator().create_hive(hive.clone())?;
                Ok(serde_json::to_value(hive)?)
            }
            ManagementRequest::CreateAgent { template, config } => {
                let agent = management.factory.create(template, config).await?;
                let id = agent.id().to_owned();
                self.register_agent(agent)?;
                Ok(serde_json::json!({"agent_id":id}))
            }
            ManagementRequest::JoinHive { hive_id, agent_id } => {
                self.coordinator().join_hive(&hive_id, &agent_id)?;
                Ok(serde_json::json!({"joined":true}))
            }
            ManagementRequest::LeaveHive { hive_id, agent_id } => {
                self.coordinator().leave_hive(&hive_id, &agent_id)?;
                Ok(serde_json::json!({"left":true}))
            }
        }
    }
}
#[cfg(test)]
mod test;
