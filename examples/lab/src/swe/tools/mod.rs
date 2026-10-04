//! The tool surface a seat is shown, rendered as OpenAI function schemas.
//!
//! Two sources, one list: `bash` is the lab's own, and the speaking tools come
//! from core's [`tool_specs`], rendered verbatim so the descriptions a seat
//! reads are exactly the contract core states. [`parse_arguments`] goes the
//! other way: a model's JSON arguments become core's [`CallArguments`].

use serde_json::{Value, json};
use tinyhivemind_core::runtime::speech::{ParameterKind, ToolSpec, tool_specs};

/// The tools a hive seat may call besides `bash`.
pub const HIVE_TOOLS: &[&str] = &["post", "broadcast", "ask", "complete_episode", "read"];

/// The only speaking tool the single-agent baseline has.
pub const SINGLE_TOOLS: &[&str] = &["complete_episode"];

/// The `bash` tool schema.
#[must_use]
pub fn bash_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "bash",
            "description": "Run a shell command in the task sandbox (no internet). \
                            Returns the merged stdout and stderr and the exit code.",
            "parameters": {
                "type": "object",
                "properties": { "cmd": { "type": "string", "description": "The command line." } },
                "required": ["cmd"]
            }
        }
    })
}

/// Render one core tool spec as a function schema.
#[must_use]
pub fn render(spec: &ToolSpec) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for parameter in spec.parameters {
        let mut schema = match parameter.kind {
            ParameterKind::Text => json!({ "type": "string" }),
            ParameterKind::TextList => json!({ "type": "array", "items": { "type": "string" } }),
            ParameterKind::Count { default, min, max } => {
                json!({ "type": "integer", "default": default, "minimum": min, "maximum": max })
            }
        };
        if let Some(description) = parameter.description {
            schema["description"] = json!(description);
        }
        properties.insert(parameter.name.to_owned(), schema);
        if parameter.required {
            required.push(parameter.name);
        }
    }
    json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": { "type": "object", "properties": properties, "required": required }
        }
    })
}

/// `bash` plus the named core tools, in core's order.
#[must_use]
pub fn tool_list(speaking: &[&str]) -> Vec<Value> {
    let mut tools = vec![bash_tool()];
    tools.extend(
        tool_specs()
            .iter()
            .filter(|spec| speaking.contains(&spec.name))
            .map(render),
    );
    tools
}

/// Owned arguments, because [`CallArguments`] borrows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arguments {
    /// `message`.
    pub message: Option<String>,
    /// `to`, as a list even when the model wrote one string.
    pub to: Vec<String>,
    /// `limit`.
    pub limit: Option<u64>,
}

/// Read a model's argument object into [`Arguments`].
#[must_use]
pub fn parse_arguments(args: &Value) -> Arguments {
    let to = match args.get("to") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(one)) => vec![one.clone()],
        _ => Vec::new(),
    };
    Arguments {
        message: args.get("message").and_then(Value::as_str).map(str::to_owned),
        to,
        limit: args.get("limit").and_then(Value::as_u64),
    }
}

#[cfg(test)]
mod test;
