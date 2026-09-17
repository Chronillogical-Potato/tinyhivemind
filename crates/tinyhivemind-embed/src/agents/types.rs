//! Opaque instantiated-agent registry and route resolution values.

use std::collections::{BTreeMap, BTreeSet};

use crate::RoutingPlan;

/// A validation failure while registering or resolving instantiated agents.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AgentRegistryError {
    /// A registry key contains no canonical agent id.
    #[error("agent id is blank")]
    BlankId,
    /// Two instantiated handles claim the same canonical agent id.
    #[error("agent id {0:?} is registered more than once")]
    DuplicateId(String),
    /// An accepted route names no instantiated handle.
    #[error("routed agent {0:?} is not instantiated")]
    MissingAgent(String),
    /// A hive route names one instantiated handle more than once.
    #[error("routed agent {0:?} appears more than once")]
    DuplicateRoutedAgent(String),
}

/// Caller-instantiated agents indexed by canonical routing id.
///
/// `A` remains completely opaque. In particular this type owns no session id:
/// a runtime-specific agent handle remains responsible for its own continuity.
#[derive(Debug)]
pub struct AgentRegistry<A> {
    agents: BTreeMap<String, A>,
}

impl<A> AgentRegistry<A> {
    /// Register already-instantiated agents under their canonical routing ids.
    ///
    /// # Errors
    ///
    /// Returns [`AgentRegistryError::BlankId`] for a blank id or
    /// [`AgentRegistryError::DuplicateId`] when an id appears twice.
    pub fn new<I, S>(agents: I) -> Result<Self, AgentRegistryError>
    where
        I: IntoIterator<Item = (S, A)>,
        S: Into<String>,
    {
        let mut by_id = BTreeMap::new();
        for (id, agent) in agents {
            let id = id.into();
            if id.trim().is_empty() {
                return Err(AgentRegistryError::BlankId);
            }
            if by_id.insert(id.clone(), agent).is_some() {
                return Err(AgentRegistryError::DuplicateId(id));
            }
        }
        Ok(Self { agents: by_id })
    }

    /// Return the instantiated agent registered under `id`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&A> {
        self.agents.get(id)
    }

    /// Resolve an accepted plan to borrowed instantiated agents.
    ///
    /// A single or fallback plan resolves one agent. A hive resolves the
    /// primary first and specialists in the plan's invitation order. A
    /// clarification resolves no agent because no turn is authorized.
    ///
    /// # Errors
    ///
    /// Returns [`AgentRegistryError::MissingAgent`] if the accepted plan names
    /// an unregistered id, or [`AgentRegistryError::DuplicateRoutedAgent`] if
    /// a malformed hive repeats one.
    pub fn resolve(&self, plan: &RoutingPlan) -> Result<RoutedAgents<'_, A>, AgentRegistryError> {
        match plan {
            RoutingPlan::One { responder_id, .. } | RoutingPlan::Fallback { responder_id, .. } => {
                Ok(RoutedAgents::One(self.routed(responder_id)?))
            }
            RoutingPlan::Hive {
                primary_id,
                invited_ids,
                ..
            } => {
                let mut seen = BTreeSet::new();
                let primary = self.unique_routed(primary_id, &mut seen)?;
                let invited = invited_ids
                    .iter()
                    .map(|id| self.unique_routed(id, &mut seen))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(RoutedAgents::Hive { primary, invited })
            }
            RoutingPlan::Clarify { .. } => Ok(RoutedAgents::Clarify),
        }
    }

    fn routed(&self, id: &str) -> Result<RoutedAgent<'_, A>, AgentRegistryError> {
        let (registered_id, agent) = self
            .agents
            .get_key_value(id)
            .ok_or_else(|| AgentRegistryError::MissingAgent(id.to_string()))?;
        Ok(RoutedAgent {
            id: registered_id,
            agent,
        })
    }

    fn unique_routed(
        &self,
        id: &str,
        seen: &mut BTreeSet<String>,
    ) -> Result<RoutedAgent<'_, A>, AgentRegistryError> {
        if !seen.insert(id.to_string()) {
            return Err(AgentRegistryError::DuplicateRoutedAgent(id.to_string()));
        }
        self.routed(id)
    }
}

/// One accepted route id paired with its existing agent instance.
#[derive(Clone, Copy, Debug)]
pub struct RoutedAgent<'a, A> {
    /// Canonical routing id.
    pub id: &'a str,
    /// The exact instance supplied by the embedding host.
    pub agent: &'a A,
}

/// Instantiated agents authorized by one accepted routing plan.
#[derive(Clone, Debug)]
pub enum RoutedAgents<'a, A> {
    /// One direct, semantic, surface-rule, or fallback responder.
    One(RoutedAgent<'a, A>),
    /// One primary plus bounded invited specialists.
    Hive {
        /// Primary responder.
        primary: RoutedAgent<'a, A>,
        /// Additional responders in accepted invitation order.
        invited: Vec<RoutedAgent<'a, A>>,
    },
    /// No agent turn is authorized until the request is clarified.
    Clarify,
}
