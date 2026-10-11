//! The seat factory: one OpenHuman agent per lowered seat, created from the
//! language adapter's `management_request` envelope.
//!
//! The factory refuses anything it cannot honour in full, as the adapter's
//! README requires of a factory: an unknown template, a seat without a
//! memory binding, a tool the host does not serve, a binding outside the
//! package root. Each agent is host-only (no harness tool reaches the model)
//! and bound through `language::memory_binding` to its run-scoped CortexDB
//! namespace, where OpenHuman logs its turns.

use std::collections::BTreeMap;
use std::sync::Arc;

use openhuman_embed::{AgentDefinitionSpec, AgentSpec, Runtime, ToolScopeSpec};
use serde_json::Value;
use tinyhivemind_lang::{LoweredMemoryBinding, ToolAccess};
use tinyhivemind_openhuman::{AgentFactory, AgentFuture, Error, language};

use super::memory::scope_binding;

/// The template every euler-hive seat names.
pub(super) const TEMPLATE: &str = "euler-seat";

/// Builds seats on one runtime.
pub(super) struct SeatFactory {
    pub(super) runtime: Arc<Runtime>,
    /// The role charter each seat's system prompt carries, by seat id.
    pub(super) charters: BTreeMap<String, String>,
    /// The hive protocol every seat's system prompt ends with.
    pub(super) protocol: String,
    pub(super) package_root: String,
    pub(super) run: String,
}

/// The system prompt one seat runs with. Pure, so it is pinned by test.
pub(super) fn system_prompt(prompt: &str, charter: &str, protocol: &str) -> String {
    format!("{}\n\n## Your role\n{}\n\n{}", prompt.trim(), charter.trim(), protocol.trim())
}

impl AgentFactory for SeatFactory {
    fn create(&self, _template: String, _config: Value) -> AgentFuture {
        Box::pin(async { Err(Error::MemoryBindingUnsupported) })
    }

    fn create_with_memory(
        &self,
        template: String,
        config: Value,
        memory: Option<LoweredMemoryBinding>,
    ) -> AgentFuture {
        let runtime = self.runtime.clone();
        let charters = self.charters.clone();
        let protocol = self.protocol.clone();
        let root = self.package_root.clone();
        let run = self.run.clone();
        Box::pin(async move {
            let refuse = |reason: &str| Error::UnboundSeat {
                seat: config["id"].as_str().unwrap_or("?").to_owned(),
                reason: reason.to_owned(),
            };
            if template != TEMPLATE {
                return Err(refuse("unknown template"));
            }
            let id = config["id"].as_str().ok_or_else(|| refuse("no seat id"))?;
            let prompt = config["prompt"].as_str().ok_or_else(|| refuse("no prompt"))?;
            let tools: ToolAccess = serde_json::from_value(config["tools"].clone())
                .map_err(|_| refuse("unreadable tools"))?;
            if tools.allow.is_empty() {
                return Err(refuse("a seat must allow its hive tools"));
            }
            let binding = memory.ok_or_else(|| refuse("every euler seat must bind memory"))?;
            if binding.settings.reads.iter().any(|reach| reach.inherit) {
                return Err(refuse("inherited reads are not served"));
            }
            let scoped = scope_binding(&binding, &root, &run)
                .map_err(|error| refuse(&error.to_string()))?;
            let charter = charters.get(id).map_or("", String::as_str);
            let iterations = config["config"]["max_iterations"].as_u64().unwrap_or(10);
            let temperature = config["config"]["temperature"].as_f64().unwrap_or(0.2);
            let definition = AgentDefinitionSpec::new()
                .tools(ToolScopeSpec::HostOnly)
                .max_iterations(usize::try_from(iterations).unwrap_or(10))
                .temperature(temperature);
            let spec = AgentSpec::new(id)
                .system_prompt(system_prompt(prompt, charter, &protocol))
                .definition(definition)
                .memory(language::memory_binding(&scoped)?);
            Ok(runtime.agent(spec)?)
        })
    }
}
