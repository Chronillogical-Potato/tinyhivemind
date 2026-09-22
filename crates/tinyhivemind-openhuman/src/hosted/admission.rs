//! The episode's tools, admitted through the host's own gate.

use async_trait::async_trait;
use openhuman_core::agent::tool_policy::{ToolPolicy, ToolPolicyDecision, ToolPolicyRequest};
use std::sync::Arc;

/// Admits the episode's tools by name and asks the host's policy about
/// everything else. With no host policy, everything else is denied.
///
/// The episode's tools act on nothing but the episode's own record, and the
/// record checks every call against the turn it was made in, so there is
/// nothing for a host's approval gate to approve. What the host's gate
/// decides about its own tools is unchanged.
pub(super) struct Admission {
    names: Vec<String>,
    host: Option<Arc<dyn ToolPolicy>>,
}

impl Admission {
    pub(super) fn new(names: Vec<String>, host: Option<Arc<dyn ToolPolicy>>) -> Self {
        Self { names, host }
    }
}

#[async_trait]
impl ToolPolicy for Admission {
    fn name(&self) -> &'static str {
        "episode_admission"
    }

    async fn check(&self, request: &ToolPolicyRequest) -> ToolPolicyDecision {
        if self.names.contains(&request.tool_name) {
            return ToolPolicyDecision::Allow;
        }
        match &self.host {
            Some(host) => host.check(request).await,
            None => ToolPolicyDecision::deny(format!(
                "`{}` is not on this seat's belt",
                request.tool_name
            )),
        }
    }
}
