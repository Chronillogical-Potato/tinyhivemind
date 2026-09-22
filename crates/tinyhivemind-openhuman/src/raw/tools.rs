//! The room's tools as a raw session's own, over the shared record.
//!
//! `tinyhivemind-tools` renders the vocabulary into tool definitions and
//! checks a call in `EpisodeTools::call`. This module takes those definitions
//! as they are -- name, description, the schema with `chat` and `parent` --
//! and wraps each in a `tinytools::Tool` whose `execute` is that same call.
//! Nothing about a tool is restated here, so an in-process seat and an MCP
//! seat read the same descriptions and the same refusals.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tinyhivemind_tools::{EpisodeTools, tool_definitions};
use tinytools::{PermissionLevel, Tool, ToolResult};

/// The served tools, bound to one seat.
#[must_use]
pub(crate) fn belt(seat: &str, tools: &Arc<EpisodeTools>) -> Vec<Box<dyn Tool>> {
    belt_with_prefix(seat, tools, "")
}

/// The served tools, bound to one seat, each named with `prefix` in front
/// of its served name.
///
/// The prefix is what the model sees and what a gate admits; the record is
/// called by the served name, so `interpret` and the refusals are unchanged.
/// A host whose own belt could share a bare name -- `read` is the likely
/// one -- keeps the two apart with it.
#[must_use]
pub(crate) fn belt_with_prefix(
    seat: &str,
    tools: &Arc<EpisodeTools>,
    prefix: &str,
) -> Vec<Box<dyn Tool>> {
    tool_definitions(&tools.seats())
        .into_iter()
        .map(|definition| {
            let served = text(&definition, "name");
            Box::new(EpisodeTool {
                seat: seat.to_owned(),
                name: format!("{prefix}{served}"),
                served,
                description: text(&definition, "description"),
                schema: definition
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(Value::Null),
                tools: Arc::clone(tools),
            }) as Box<dyn Tool>
        })
        .collect()
}

fn text(definition: &Value, key: &str) -> String {
    definition
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

struct EpisodeTool {
    seat: String,
    /// The name the model calls it by: the served name, prefixed.
    name: String,
    /// The served name, which the record knows it by.
    served: String,
    description: String,
    schema: Value,
    tools: Arc<EpisodeTools>,
}

#[async_trait]
impl Tool for EpisodeTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> Value {
        self.schema.clone()
    }

    /// `read` looks; everything else moves the episode.
    fn permission_level(&self) -> PermissionLevel {
        if self.served == "read" {
            PermissionLevel::ReadOnly
        } else {
            PermissionLevel::Write
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        Ok(match self.tools.call(&self.seat, &self.served, &args) {
            Ok(receipt) => ToolResult::success(receipt),
            Err(refusal) => ToolResult::error(refusal),
        })
    }
}
