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
pub(super) fn belt(seat: &str, tools: &Arc<EpisodeTools>) -> Vec<Box<dyn Tool>> {
    tool_definitions(&tools.seats())
        .into_iter()
        .map(|definition| {
            Box::new(EpisodeTool {
                seat: seat.to_owned(),
                name: text(&definition, "name"),
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
    name: String,
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
        if self.name == "read" {
            PermissionLevel::ReadOnly
        } else {
            PermissionLevel::Write
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        Ok(match self.tools.call(&self.seat, &self.name, &args) {
            Ok(receipt) => ToolResult::success(receipt),
            Err(refusal) => ToolResult::error(refusal),
        })
    }
}
