//! The OpenAI chat-completions wire, as pure functions over JSON values.
//!
//! Nothing here does I/O: building a request body and parsing a response body
//! are the parts of a model call that can be tested without a server.

use serde_json::{Value, json};

/// One tool call the model made.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolUse {
    /// The provider's call id, echoed back in the tool result.
    pub id: String,
    /// The tool name.
    pub name: String,
    /// The arguments, or an error string when the model wrote invalid JSON.
    pub args: Result<Value, String>,
}

/// One parsed model reply.
#[derive(Clone, Debug, PartialEq)]
pub struct Completion {
    /// Text outside tool calls; empty when the model wrote none.
    pub content: String,
    /// The tool calls, in order.
    pub tool_calls: Vec<ToolUse>,
    /// Prompt tokens as the provider reported them (or estimated).
    pub input_tokens: u64,
    /// Completion tokens as the provider reported them (or estimated).
    pub output_tokens: u64,
    /// The assistant message to append to the conversation verbatim.
    pub message: Value,
}

/// Build the JSON body of one chat-completions request.
#[must_use]
pub fn request_body(model: &str, messages: &[Value], tools: &[Value]) -> Value {
    let mut body = json!({ "model": model, "messages": messages });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
        body["tool_choice"] = json!("auto");
    }
    body
}

/// Parse a response body.
///
/// # Errors
///
/// Returns a message when the body carries an `error` object or no choice.
pub fn parse_response(body: &Value) -> Result<Completion, String> {
    if let Some(error) = body.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| error.to_string(), str::to_owned);
        return Err(format!("provider error: {text}"));
    }
    let message = body
        .pointer("/choices/0/message")
        .ok_or_else(|| "response has no choices".to_owned())?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let tool_calls: Vec<ToolUse> = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|calls| calls.iter().map(parse_call).collect())
        .unwrap_or_default();
    let usage = body.get("usage");
    let reported = |key: &str| usage.and_then(|u| u.get(key)).and_then(Value::as_u64);
    let input_tokens = reported("prompt_tokens").unwrap_or(0);
    let output_tokens = reported("completion_tokens")
        .unwrap_or_else(|| estimate(&content) + estimate_calls(&tool_calls));
    let mut kept = json!({ "role": "assistant", "content": content.clone() });
    if let Some(calls) = message.get("tool_calls").filter(|c| !c.is_null()) {
        kept["tool_calls"] = calls.clone();
    }
    Ok(Completion {
        content,
        tool_calls,
        input_tokens,
        output_tokens,
        message: kept,
    })
}

fn parse_call(call: &Value) -> ToolUse {
    let function = call.get("function").cloned().unwrap_or(Value::Null);
    let raw = function.get("arguments");
    let args = match raw {
        Some(Value::String(text)) if text.trim().is_empty() => Ok(json!({})),
        Some(Value::String(text)) => {
            serde_json::from_str(text).map_err(|error| format!("invalid arguments JSON: {error}"))
        }
        Some(Value::Object(_)) => Ok(raw.cloned().unwrap_or(Value::Null)),
        _ => Ok(json!({})),
    };
    ToolUse {
        id: call
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("call")
            .to_owned(),
        name: function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        args,
    }
}

/// Four characters to the token: only a fallback when usage is missing.
fn estimate(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

fn estimate_calls(calls: &[ToolUse]) -> u64 {
    calls
        .iter()
        .map(|call| estimate(&call.name) + estimate(&format!("{:?}", call.args)))
        .sum()
}

/// A `tool` role message answering `call_id`.
#[must_use]
pub fn tool_result(call_id: &str, content: &str) -> Value {
    json!({ "role": "tool", "tool_call_id": call_id, "content": content })
}

/// Escape text for a double-quoted value in a curl config file.
#[must_use]
pub fn config_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0b}' => out.push_str("\\v"),
            other => out.push(other),
        }
    }
    out
}
