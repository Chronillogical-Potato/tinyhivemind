//! `tool_specs()` as MCP tool definitions, and MCP arguments back onto
//! `CallArguments`.
//!
//! Everything a seat reads about a tool -- its name, what it does, what it
//! takes -- comes from `tinyhivemind::speech`, verbatim. This module adds
//! exactly two arguments to every tool, `chat` and `parent`, because the server
//! checks each call against the turn the host registered, and the seat has to
//! say which turn it thinks it is in for that check to mean anything.

use serde_json::{Map, Value, json};
use tinyhivemind::speech::{CallArguments, ParameterKind, ToolSpec, tool_specs};

/// The one vocabulary tool this server does not serve.
///
/// In a completion episode an `ask` is the private message that means
/// something; offering `dm` beside it would give a seat two ways to say
/// nearly the same thing.
const UNSERVED: &[&str] = &["dm"];

/// The specs this server serves, in the order a seat should meet them.
pub(crate) fn served() -> impl Iterator<Item = &'static ToolSpec> {
    tool_specs()
        .iter()
        .filter(|spec| !UNSERVED.contains(&spec.name))
}

/// Whether a tool of this name is served.
pub(crate) fn serves(name: &str) -> bool {
    served().any(|spec| spec.name == name)
}

/// Every served tool as an MCP tool definition.
///
/// `seats` are the choices `ask`'s `to` offers: a seat that can read the
/// alternatives does not guess eight ids and learn nothing from eight refusals.
pub(crate) fn tool_definitions(seats: &[String]) -> Vec<Value> {
    served().map(|spec| definition(spec, seats)).collect()
}

fn definition(spec: &ToolSpec, seats: &[String]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for parameter in spec.parameters {
        let mut schema = match parameter.kind {
            ParameterKind::Text => json!({ "type": "string" }),
            ParameterKind::TextList => json!({ "type": "array", "items": { "type": "string" } }),
            ParameterKind::Count { default, min, max } => json!({
                "type": "integer",
                "minimum": min,
                "maximum": max,
                "default": default,
            }),
        };
        if let Some(description) = parameter.description {
            schema["description"] = Value::String(description.to_owned());
        }
        if spec.name == "ask" && parameter.name == "to" {
            schema["enum"] = Value::Array(seats.iter().cloned().map(Value::String).collect());
        }
        properties.insert(parameter.name.to_owned(), schema);
        if parameter.required {
            required.push(Value::String(parameter.name.to_owned()));
        }
    }
    properties.insert(
        "chat".to_owned(),
        json!({
            "type": "string",
            "description": "The chat this turn is in, exactly as you were told at the top of your turn.",
        }),
    );
    properties.insert(
        "parent".to_owned(),
        json!({
            "type": ["string", "null"],
            "description": "The thread root you were told, or null when the turn is in the chat itself.",
        }),
    );
    required.push(Value::String("chat".to_owned()));
    json!({
        "name": spec.name,
        "description": spec.description,
        "inputSchema": {
            "type": "object",
            "properties": Value::Object(properties),
            "required": required,
        },
    })
}

/// The arguments of one call, owned, in the shapes the specs declare.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Arguments {
    pub message: Option<String>,
    pub to: Vec<String>,
    pub limit: Option<u64>,
    pub chat: Option<String>,
    pub parent: Option<String>,
}

impl Arguments {
    /// Borrow as the algebra's argument shape.
    pub(crate) fn call(&self) -> CallArguments<'_> {
        CallArguments {
            message: self.message.as_deref(),
            to: &self.to,
            limit: self.limit,
        }
    }
}

/// Read a call's arguments, however the dispatcher chose to send them.
///
/// A generic dispatcher may forward them as an object, as a JSON string, or
/// flattened onto the params themselves, and every one of those looks
/// identical from inside a refusal. Accepting all three is cheaper than being
/// wrong about which. `to` is one string for `ask` and a list for anything
/// that takes several; both are read.
pub(crate) fn arguments(params: &Value) -> Arguments {
    let raw = match params.get("arguments") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_else(|_| params.clone()),
        _ => params.clone(),
    };
    let to = match raw.get("to") {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Array(many)) => many
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    Arguments {
        message: raw
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned),
        to,
        limit: raw.get("limit").and_then(Value::as_u64),
        chat: raw.get("chat").and_then(Value::as_str).map(str::to_owned),
        parent: raw.get("parent").and_then(Value::as_str).map(str::to_owned),
    }
}

#[cfg(test)]
mod test;
