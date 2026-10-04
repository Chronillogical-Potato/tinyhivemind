//! Local MCP surface for completion-driven hive events.

use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use super::memory_support::{Compaction, MarkdownMemory};
use serde_json::{Value, json};
use tinyhivemind_core::runtime::speech::{
    CallArguments, ParameterKind, ToolCall, Utterance, interpret, tool_specs,
};
use tinyhivemind_tools::{MemoryTools, memory_tool_definitions};

#[derive(Clone, Debug)]
pub(super) struct Server {
    pub agent_id: String,
    pub outbox: PathBuf,
    /// The shared markdown memory, when the hive serves one.
    pub memory: Option<PathBuf>,
}

pub(super) fn requested() -> anyhow::Result<Option<Server>> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--hive-tools") {
        return Ok(None);
    }
    let mut agent_id = None;
    let mut outbox = None;
    let mut memory = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--agent" => agent_id = args.next(),
            "--outbox" => outbox = args.next().map(PathBuf::from),
            "--memory" => memory = args.next().map(PathBuf::from),
            _ => anyhow::bail!("unknown hive-tools argument {flag}"),
        }
    }
    Ok(Some(Server {
        agent_id: agent_id.ok_or_else(|| anyhow::anyhow!("missing --agent"))?,
        outbox: outbox.ok_or_else(|| anyhow::anyhow!("missing --outbox"))?,
        memory,
    }))
}

pub(super) fn serve(server: &Server) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let response = match method {
            "initialize" => ok(
                &id,
                &json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "tinyhive", "version": env!("CARGO_PKG_VERSION")},
                }),
            ),
            "ping" => ok(&id, &json!({})),
            "tools/list" => ok(&id, &json!({"tools": descriptors(server)})),
            "tools/call" => match call(&request, server) {
                Ok(text) => ok(&id, &json!({"content": [{"type":"text", "text":text}]})),
                Err(text) => ok(
                    &id,
                    &json!({"isError":true, "content":[{"type":"text", "text":text}]}),
                ),
            },
            other => json!({
                "jsonrpc":"2.0", "id":id,
                "error":{"code":-32601, "message":format!("unknown method {other}")}
            }),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

pub(super) fn clear(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, "")?;
    Ok(())
}

pub(super) fn drain(path: &Path) -> anyhow::Result<Vec<Utterance>> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    Ok(text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()?)
}

fn call(request: &Value, server: &Server) -> Result<String, String> {
    let name = request
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if MemoryTools::serves(name) {
        return call_memory(request, server, name);
    }
    if !matches!(name, "broadcast" | "complete_episode") {
        return Err(format!("unknown completion-hive tool {name}"));
    }
    let message = request
        .pointer("/params/arguments/message")
        .and_then(Value::as_str);
    let call = interpret(
        name,
        &CallArguments {
            message,
            ..Default::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let ToolCall::Speak(utterance) = call else {
        return Err("completion hive accepts only speaking tools".into());
    };
    let entry = serde_json::to_string(&utterance).map_err(|error| error.to_string())?;
    let mut outbox = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&server.outbox)
        .map_err(|error| error.to_string())?;
    writeln!(outbox, "{entry}").map_err(|error| error.to_string())?;
    Ok(format!("accepted from @{}", server.agent_id))
}

/// A hive memory tool, answered over the shared markdown file. Each seat's
/// server is its own process, so the engine is rebuilt from the path per call.
fn call_memory(request: &Value, server: &Server, name: &str) -> Result<String, String> {
    let path = server.memory.as_ref().ok_or("this hive serves no memory")?;
    let tools = MemoryTools::new(std::sync::Arc::new(MarkdownMemory::new(
        path,
        Compaction::DEFAULT,
    )));
    let arguments = request
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    futures::executor::block_on(tools.call(&server.agent_id, name, &arguments))
}

fn descriptors(server: &Server) -> Vec<Value> {
    let memory = server.memory.is_some().then(memory_tool_definitions);
    tool_specs()
        .iter()
        .filter(|spec| matches!(spec.name, "broadcast" | "complete_episode"))
        .map(|spec| {
            json!({
                "name":spec.name,
                "description":spec.description,
                "inputSchema":schema(spec.parameters),
            })
        })
        .chain(memory.into_iter().flatten().map(|tool| {
            json!({
                "name":tool.name,
                "description":tool.description,
                "inputSchema":tool.parameters,
            })
        }))
        .collect()
}

fn schema(parameters: &[tinyhivemind_core::runtime::speech::ToolParameter]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for parameter in parameters {
        let kind = match parameter.kind {
            ParameterKind::Text => json!({"type":"string"}),
            ParameterKind::TextList => json!({"type":"array", "items":{"type":"string"}}),
            ParameterKind::Count { .. } => json!({"type":"integer"}),
        };
        properties.insert(parameter.name.into(), kind);
        if parameter.required {
            required.push(Value::String(parameter.name.into()));
        }
    }
    json!({"type":"object", "properties":properties, "required":required})
}

fn ok(id: &Value, result: &Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}
