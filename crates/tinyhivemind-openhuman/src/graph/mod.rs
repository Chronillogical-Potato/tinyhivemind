//! One owned desk graph and its `OpenHuman` bindings.

#[cfg(test)]
mod test;

use std::collections::{BTreeMap, BTreeSet};

use openhuman_embed::Agent;
use tinyhivemind::desk::{Desk, DeskSet};
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, MessageRoute, RouteCandidate, Router, RoutingPlan,
    RoutingPolicy, RoutingRequest, RoutingSource, route_message,
};

use crate::{Error, Result};

/// An owned one-desk routing graph.
#[derive(Clone, Debug)]
pub struct HiveGraph {
    /// The one desk represented by this hive.
    pub desk: Desk,
    /// Semantic routing candidates, one for every desk member.
    pub candidates: Vec<RouteCandidate>,
}

impl HiveGraph {
    /// Build an owned graph. Structural validation occurs in [`OpenHumanHive::new`].
    #[must_use]
    pub const fn new(desk: Desk, candidates: Vec<RouteCandidate>) -> Self {
        Self { desk, candidates }
    }
}

/// One canonical hive id bound to a concrete `OpenHuman` agent handle.
///
/// The canonical id deliberately need not equal [`Agent::id`]. Cloning a
/// binding is cheap and permits the same runtime agent to participate in more
/// than one hive without changing either identity.
#[derive(Clone, Debug)]
pub struct AgentBinding {
    /// Canonical id used by the desk and routing graph.
    pub hive_agent_id: String,
    /// Existing runtime-owned `OpenHuman` agent.
    pub agent: Agent,
}

impl AgentBinding {
    /// Bind a canonical hive id to an existing `OpenHuman` agent.
    #[must_use]
    pub fn new(hive_agent_id: impl Into<String>, agent: Agent) -> Self {
        Self {
            hive_agent_id: hive_agent_id.into(),
            agent,
        }
    }

    /// Return the `OpenHuman` runtime id, which may differ from the hive id.
    #[must_use]
    pub fn runtime_agent_id(&self) -> &str {
        self.agent.id()
    }
}

/// A validated graph plus its already-instantiated `OpenHuman` agents.
#[derive(Clone, Debug)]
pub struct OpenHumanHive {
    graph: HiveGraph,
    bindings: BTreeMap<String, AgentBinding>,
}

impl OpenHumanHive {
    /// Validate and own one complete `OpenHuman` hive.
    ///
    /// # Errors
    ///
    /// Returns a typed error for malformed desk data, empty or repeated
    /// membership, repeated candidate or binding ids, or any candidate/binding
    /// set that is not exactly equal to the desk membership set.
    pub fn new(graph: HiveGraph, bindings: Vec<AgentBinding>) -> Result<Self> {
        validate_ids("members", graph.desk.members.iter().map(String::as_str))?;
        validate_ids(
            "candidates",
            graph
                .candidates
                .iter()
                .map(|candidate| candidate.id.as_str()),
        )?;
        validate_ids(
            "bindings",
            bindings
                .iter()
                .map(|binding| binding.hive_agent_id.as_str()),
        )?;
        let declared = std::slice::from_ref(&graph.desk);
        DeskSet::new(declared, &[], &[], &[], &[]).validate()?;
        if graph.desk.members.is_empty() {
            return Err(Error::EmptyMembership {
                desk_id: graph.desk.id.clone(),
            });
        }
        let members = unique_ids(graph.desk.members.iter().map(String::as_str), |agent_id| {
            Error::DuplicateMemberId { agent_id }
        })?;
        let candidate_ids = unique_ids(
            graph
                .candidates
                .iter()
                .map(|candidate| candidate.id.as_str()),
            |agent_id| Error::DuplicateCandidateId { agent_id },
        )?;
        require_same_set("candidates", &members, &candidate_ids)?;

        let binding_ids = unique_ids(
            bindings
                .iter()
                .map(|binding| binding.hive_agent_id.as_str()),
            |agent_id| Error::DuplicateBindingId { agent_id },
        )?;
        require_same_set("bindings", &members, &binding_ids)?;
        let bindings = bindings
            .into_iter()
            .map(|binding| (binding.hive_agent_id.clone(), binding))
            .collect();
        Ok(Self { graph, bindings })
    }

    /// Return the complete owned graph.
    #[must_use]
    pub const fn graph(&self) -> &HiveGraph {
        &self.graph
    }

    /// Return this hive's sole desk.
    #[must_use]
    pub const fn desk(&self) -> &Desk {
        &self.graph.desk
    }

    /// Return canonical members in desk order.
    pub fn members(&self) -> impl ExactSizeIterator<Item = &str> {
        self.graph.desk.members.iter().map(String::as_str)
    }

    /// Look up the concrete `OpenHuman` agent bound to a canonical hive id.
    #[must_use]
    pub fn bound_agent(&self, hive_agent_id: &str) -> Option<&Agent> {
        self.bindings
            .get(hive_agent_id)
            .map(|binding| &binding.agent)
    }

    /// Look up the full binding for a canonical hive id.
    #[must_use]
    pub fn binding(&self, hive_agent_id: &str) -> Option<&AgentBinding> {
        self.bindings.get(hive_agent_id)
    }

    /// Build a desk-scoped request from this graph's immutable candidates.
    #[must_use]
    pub fn desk_request(
        &self,
        message: impl Into<String>,
        thread_context: Vec<String>,
        thread_root: Option<tinyhivemind::Sequence>,
        roster_version: u64,
        policy: RoutingPolicy,
    ) -> RoutingRequest {
        RoutingRequest {
            message: message.into(),
            source: RoutingSource::DeskMessage,
            conversation: ConversationRef {
                id: self.graph.desk.id.clone(),
                kind: ConversationKind::Desk,
                thread_root,
            },
            desk_purpose: self.graph.desk.description.clone(),
            thread_context,
            candidates: self.graph.candidates.clone(),
            roster_version,
            policy,
        }
    }

    /// Route a desk message through the existing deterministic-bypass API.
    ///
    /// An explicit mention is passed unchanged to [`route_message`] and thus
    /// bypasses both semantic routers. Before routing, the request's desk,
    /// surface, source, purpose, and ordered candidate snapshot must exactly
    /// match this hive, and both deterministic responder ids must be members.
    ///
    /// # Errors
    ///
    /// Returns a typed graph mismatch or unknown-agent error before invoking a
    /// router.
    pub async fn route_desk(
        &self,
        primary: Option<&(dyn Router + '_)>,
        reasoning: Option<&(dyn Router + '_)>,
        request: &RoutingRequest,
        explicit_responder: Option<&str>,
        fallback_responder: &str,
    ) -> Result<RoutingPlan> {
        for (matches, field) in [
            (
                request.conversation.id == self.graph.desk.id,
                "conversation.id",
            ),
            (
                request.conversation.kind == ConversationKind::Desk,
                "conversation.kind",
            ),
            (request.source == RoutingSource::DeskMessage, "source"),
            (
                request.desk_purpose == self.graph.desk.description,
                "desk_purpose",
            ),
            (request.candidates == self.graph.candidates, "candidates"),
        ] {
            if !matches {
                return Err(Error::MismatchedDeskRoutingRequest { field });
            }
        }
        for responder_id in explicit_responder.into_iter().chain([fallback_responder]) {
            if self.binding(responder_id).is_none() {
                return Err(Error::UnknownBoundAgent {
                    agent_id: responder_id.to_owned(),
                });
            }
        }
        Ok(route_message(
            primary,
            reasoning,
            request,
            explicit_responder,
            fallback_responder,
        )
        .await)
    }

    /// Resolve an accepted plan to bound agents in primary-then-invited order.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownBoundAgent`] if the plan names an outsider.
    pub fn resolve_plan(&self, plan: &RoutingPlan) -> Result<Vec<&AgentBinding>> {
        let ids: Vec<&str> = match plan {
            RoutingPlan::One { responder_id, .. } | RoutingPlan::Fallback { responder_id, .. } => {
                vec![responder_id]
            }
            RoutingPlan::Hive {
                primary_id,
                invited_ids,
                ..
            } => std::iter::once(primary_id.as_str())
                .chain(invited_ids.iter().map(String::as_str))
                .collect(),
            RoutingPlan::Clarify { .. } => Vec::new(),
        };
        ids.into_iter()
            .map(|id| {
                self.binding(id).ok_or_else(|| Error::UnknownBoundAgent {
                    agent_id: id.to_owned(),
                })
            })
            .collect()
    }

    /// Resolve a hive-scoped private message.
    ///
    /// Both sender and every recipient must be desk members. The returned
    /// route is always a desk aside and therefore cannot escape this desk.
    ///
    /// # Errors
    ///
    /// Returns a typed membership, duplication, or width error.
    pub fn resolve_dm(
        &self,
        sender_id: &str,
        recipient_ids: &[String],
        round_width: usize,
    ) -> Result<MessageRoute> {
        let members: BTreeSet<_> = self.members().collect();
        if !members.contains(sender_id) {
            return Err(Error::UnknownDmSender {
                agent_id: sender_id.to_owned(),
            });
        }
        if recipient_ids.is_empty() {
            return Err(Error::EmptyDmRecipients);
        }
        let mut seen = BTreeSet::new();
        for id in recipient_ids {
            if !members.contains(id.as_str()) {
                return Err(Error::UnknownDmRecipient {
                    agent_id: id.clone(),
                });
            }
            if !seen.insert(id.as_str()) {
                return Err(Error::DuplicateDmRecipient {
                    agent_id: id.clone(),
                });
            }
        }
        MessageRoute::desk_aside(recipient_ids.to_vec(), round_width).map_err(|_| {
            Error::DmTooWide {
                recipient_count: recipient_ids.len(),
                round_width,
            }
        })
    }
}

fn validate_ids<'a>(
    collection: &'static str,
    ids: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            return Err(Error::BlankCanonicalId { collection });
        }
        if id == "none" {
            return Err(Error::ReservedCanonicalId { collection });
        }
    }
    Ok(())
}

fn unique_ids<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    duplicate: impl Fn(String) -> Error,
) -> Result<BTreeSet<String>> {
    let mut unique = BTreeSet::new();
    for id in ids {
        if !unique.insert(id.to_owned()) {
            return Err(duplicate(id.to_owned()));
        }
    }
    Ok(unique)
}

fn require_same_set(
    collection: &'static str,
    members: &BTreeSet<String>,
    actual: &BTreeSet<String>,
) -> Result<()> {
    let missing = members.difference(actual).cloned().collect::<Vec<_>>();
    let outsiders = actual.difference(members).cloned().collect::<Vec<_>>();
    if missing.is_empty() && outsiders.is_empty() {
        Ok(())
    } else {
        Err(Error::MembershipMismatch {
            collection,
            missing,
            outsiders,
        })
    }
}
