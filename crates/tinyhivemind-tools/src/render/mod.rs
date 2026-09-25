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

/// The vocabulary tools this server does not serve.
///
/// In a completion episode every call has a consequence: `ask` opens a
/// question, `broadcast` hands work off, `complete_episode` concludes and its
/// message is the finding. `post` is text with no consequence, and five live
/// runs used it for status, for restating a finding the seat then completed
/// with anyway, and for describing calls it had not made. `dm` beside `ask`
/// is two ways to say nearly the same thing.
const UNSERVED: &[&str] = &["dm", "post"];

/// The specs this server serves, in the order a seat should meet them.
pub(crate) fn served() -> impl Iterator<Item = &'static ToolSpec> {
    tool_specs()
        .iter()
        .filter(|spec| !UNSERVED.contains(&spec.name))
}

/// The specs this server serves, for a host that renders the standing
/// contract from the same list the seats are offered.
///
/// A host that withholds a tool renders its contract from
/// [`EpisodeTools::specs`](crate::EpisodeTools::specs) instead, which is this
/// list minus what it withheld.
pub fn served_specs() -> impl Iterator<Item = &'static ToolSpec> {
    served()
}

/// Whether this tool's `to` names seats of the desk, and so is offered the
/// roster as its choices.
pub(crate) fn names_a_seat(tool: &str) -> bool {
    matches!(tool, "ask" | "ask_teammates")
}

/// Whether a tool of this name is served.
pub(crate) fn serves(name: &str) -> bool {
    served().any(|spec| spec.name == name)
}

/// Every served tool as an MCP tool definition.
///
/// `seats` are the choices the asking tools' `to` offers: a seat that can read
/// the alternatives does not guess eight ids and learn nothing from eight
/// refusals.
/// The served tools as MCP tool definitions: name, description and an
/// `inputSchema` that carries the vocabulary's parameters plus the `chat` and
/// `parent` every call must name. `seats` fills `ask`'s recipient enumeration.
///
/// Public so an in-process host can render the same definitions into its own
/// tool language instead of re-stating the schema.
#[must_use]
pub fn tool_definitions(seats: &[String]) -> Vec<Value> {
    let seats: Vec<(String, String)> = seats
        .iter()
        .map(|seat| (seat.clone(), seat.clone()))
        .collect();
    named_definitions(&seats)
}

/// [`tool_definitions`] over `(id, name)` pairs: the recipient enumeration
/// is the ids, and where any seat has a name of its own the recipient's
/// description pairs each name with its id.
pub(crate) fn named_definitions(seats: &[(String, String)]) -> Vec<Value> {
    served().map(|spec| definition(spec, seats)).collect()
}

fn definition(spec: &ToolSpec, seats: &[(String, String)]) -> Value {
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
        if names_a_seat(spec.name) && parameter.name == "to" {
            let ids = Value::Array(
                seats
                    .iter()
                    .map(|(id, _)| Value::String(id.clone()))
                    .collect(),
            );
            // `ask` takes a list of seats, so the enumeration constrains each
            // entry rather than the argument. Written against the rendered
            // shape rather than the parameter's kind: whichever `ask`'s `to`
            // becomes, the choices land where a client reads them.
            if let Some(items) = schema.get_mut("items") {
                items["enum"] = ids;
            } else {
                schema["enum"] = ids;
            }
            if let Some(roster) = roster(seats) {
                let base = parameter.description.unwrap_or_default();
                schema["description"] = Value::String(format!("{base} {roster}"));
            }
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

/// Each seat by its name and id, or by its id alone when it has no name;
/// `None` when no seat has a name of its own.
fn roster(seats: &[(String, String)]) -> Option<String> {
    if seats.iter().all(|(id, name)| id == name) {
        return None;
    }
    let listed: Vec<String> = seats
        .iter()
        .map(|(id, name)| {
            if id == name {
                id.clone()
            } else {
                format!("{name} (id `{id}`)")
            }
        })
        .collect();
    Some(format!("On this desk: {}.", listed.join(", ")))
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
/// wrong about which. `to` is a list for every tool that takes it, and a bare
/// string is read as a list of one: a model that asks a single seat writes it
/// both ways.
#[cfg(test)]
pub(crate) fn arguments(params: &Value) -> Arguments {
    parse_arguments(&raw_arguments(params))
}

/// The `arguments` object of a `tools/call`, whether the client sent it as an
/// object or as a JSON-encoded string; the params themselves when it sent
/// neither.
///
/// Public for the MCP server, which frames a `tools/call` over
/// [`EpisodeTools::call`](crate::EpisodeTools::call) and hands it this.
#[must_use]
pub fn raw_arguments(params: &Value) -> Value {
    match params.get("arguments") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_else(|_| params.clone()),
        _ => params.clone(),
    }
}

/// One call's arguments read off its object.
pub(crate) fn parse_arguments(raw: &Value) -> Arguments {
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
