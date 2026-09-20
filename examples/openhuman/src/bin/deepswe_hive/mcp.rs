//! Stdio MCP servers for Docker-confined workspace access and hive events.

use std::fs::OpenOptions;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tinyhivemind::speech::{CallArguments, ToolCall, Utterance, interpret};

use super::sandbox::{DockerSandbox, SandboxConfig};

const MAX_OUTBOX_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
pub(super) enum Server {
    Hive { agent: String, outbox: PathBuf },
    Workspace { sandbox: DockerSandbox },
}

enum CallError {
    InvalidParams(String),
    Execution(String),
}

pub(super) fn requested() -> anyhow::Result<Option<Server>> {
    let mut args = std::env::args().skip(1);
    let Some(mode) = args.next() else {
        return Ok(None);
    };
    if !matches!(mode.as_str(), "--mcp-hive" | "--mcp-workspace") {
        return Ok(None);
    }
    let mut values = std::collections::BTreeMap::new();
    while let Some(flag) = args.next() {
        values.insert(
            flag,
            args.next()
                .ok_or_else(|| anyhow::anyhow!("missing MCP value"))?,
        );
    }
    if mode == "--mcp-hive" {
        return Ok(Some(Server::Hive {
            agent: take(&mut values, "--agent")?,
            outbox: take(&mut values, "--outbox")?.into(),
        }));
    }
    let config = SandboxConfig {
        repo_path: take(&mut values, "--repo")?.into(),
        image: take(&mut values, "--image")?,
        docker: take(&mut values, "--docker")?.into(),
    };
    let dot_git_is_file = match take(&mut values, "--dot-git-kind")?.as_str() {
        "file" => true,
        "directory" => false,
        _ => anyhow::bail!("invalid --dot-git-kind"),
    };
    let git_dir = take(&mut values, "--git-dir")?.into();
    let common_dir = take(&mut values, "--git-common-dir")?.into();
    Ok(Some(Server::Workspace {
        sandbox: DockerSandbox::preflight_from_parent(
            config,
            dot_git_is_file,
            git_dir,
            common_dir,
        )?,
    }))
}

pub(super) fn serve(server: &Server) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(request) = serde_json::from_str::<Value>(&line?) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let response = response(server, &request, id);
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

pub(super) fn response(server: &Server, request: &Value, id: Value) -> Value {
    match request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "initialize" => success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": server.name(), "version": env!("CARGO_PKG_VERSION")}
            }),
        ),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({"tools": descriptors(server)})),
        "tools/call" => match call(server, request) {
            Ok(text) => success(id, json!({"content": [{"type": "text", "text": text}]})),
            Err(CallError::InvalidParams(error)) => json!({
                "jsonrpc":"2.0", "id":id,
                "error":{"code":-32602,"message":error}
            }),
            Err(CallError::Execution(error)) => json!({
                "jsonrpc":"2.0", "id":id,
                "result":{"content":[{"type":"text","text":error}],"isError":true}
            }),
        },
        other => json!({
            "jsonrpc":"2.0", "id":id,
            "error":{"code":-32601,"message":format!("unknown method {other}")}
        }),
    }
}

fn success(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}

impl Server {
    fn name(&self) -> &'static str {
        match self {
            Self::Hive { .. } => "tinyhive",
            Self::Workspace { .. } => "deepswe",
        }
    }
}

fn call(server: &Server, request: &Value) -> Result<String, CallError> {
    let params = request
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| CallError::InvalidParams("tools/call requires params".into()))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| CallError::InvalidParams("tools/call requires a tool name".into()))?;
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or_else(|| CallError::InvalidParams("tools/call requires object arguments".into()))?;
    let required = match server {
        Server::Hive { .. } => match name {
            "broadcast" | "complete_episode" => ["message"].as_slice(),
            _ => {
                return Err(CallError::InvalidParams(format!(
                    "unknown hive tool {name}"
                )));
            }
        },
        Server::Workspace { .. } => match name {
            "file_read" => ["path"].as_slice(),
            "file_write" => ["path", "content"].as_slice(),
            "file_edit" => ["path", "old", "new"].as_slice(),
            "shell" | "test" => ["command"].as_slice(),
            _ => {
                return Err(CallError::InvalidParams(format!(
                    "unknown workspace tool {name}"
                )));
            }
        },
    };
    for field in required {
        if !args.get(*field).is_some_and(Value::is_string) {
            return Err(CallError::InvalidParams(format!("missing {field}")));
        }
    }
    let args = Value::Object(args.clone());
    match server {
        Server::Hive { agent, outbox } => {
            hive_call(agent, outbox, name, &args).map_err(CallError::Execution)
        }
        Server::Workspace { sandbox } => {
            workspace_call(sandbox, name, &args).map_err(CallError::Execution)
        }
    }
}

fn hive_call(agent: &str, outbox: &Path, name: &str, args: &Value) -> Result<String, String> {
    if !matches!(name, "broadcast" | "complete_episode") {
        return Err(format!("unknown hive tool {name}"));
    }
    let call = interpret(
        name,
        &CallArguments {
            message: args.get("message").and_then(Value::as_str),
            ..Default::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let ToolCall::Speak(utterance) = call else {
        return Err("hive server accepts only speaking tools".into());
    };
    let metadata = std::fs::symlink_metadata(outbox).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("hive outbox must be a prepared regular file".into());
    }
    let mut file = OpenOptions::new()
        .append(true)
        .open(outbox)
        .map_err(|error| error.to_string())?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&utterance).map_err(|e| e.to_string())?
    )
    .map_err(|error| error.to_string())?;
    Ok(format!("accepted from @{agent}"))
}

fn workspace_call(sandbox: &DockerSandbox, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "file_read" => sandbox
            .file_read(string_arg(args, "path")?)
            .map_err(|error| error.to_string()),
        "file_write" => {
            sandbox
                .file_write(string_arg(args, "path")?, string_arg(args, "content")?)
                .map_err(|error| error.to_string())?;
            Ok("written".into())
        }
        "file_edit" => {
            sandbox
                .file_edit(
                    string_arg(args, "path")?,
                    string_arg(args, "old")?,
                    string_arg(args, "new")?,
                )
                .map_err(|error| error.to_string())?;
            Ok("edited".into())
        }
        "shell" | "test" => {
            let output = sandbox
                .shell(string_arg(args, "command")?)
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "exit_code={:?}\nstdout:\n{}\nstderr:\n{}",
                output.code,
                truncate(&output.stdout),
                truncate(&output.stderr)
            ))
        }
        _ => Err(format!("unknown workspace tool {name}")),
    }
}

fn descriptors(server: &Server) -> Vec<Value> {
    match server {
        Server::Hive { .. } => ["broadcast", "complete_episode"]
            .into_iter()
            .map(|name| descriptor(name, "Publish one completion-episode event", &["message"]))
            .collect(),
        Server::Workspace { .. } => vec![
            descriptor("file_read", "Read a repository file", &["path"]),
            descriptor(
                "file_write",
                "Write a repository file",
                &["path", "content"],
            ),
            descriptor(
                "file_edit",
                "Replace one exact substring",
                &["path", "old", "new"],
            ),
            descriptor(
                "shell",
                "Run a command in the no-network Docker sandbox",
                &["command"],
            ),
            descriptor(
                "test",
                "Run tests in the no-network Docker sandbox",
                &["command"],
            ),
        ],
    }
}

fn descriptor(name: &str, description: &str, fields: &[&str]) -> Value {
    let properties = fields
        .iter()
        .map(|field| ((*field).to_string(), json!({"type":"string"})))
        .collect::<serde_json::Map<_, _>>();
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type":"object", "properties":properties, "required":fields}
    })
}

fn string_arg<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {name}"))
}

fn truncate(value: &str) -> &str {
    if value.len() <= 100_000 {
        return value;
    }
    let end = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= 100_000)
        .last()
        .unwrap_or(0);
    &value[..end]
}

fn take(
    values: &mut std::collections::BTreeMap<String, String>,
    key: &str,
) -> anyhow::Result<String> {
    values
        .remove(key)
        .ok_or_else(|| anyhow::anyhow!("missing {key}"))
}

pub(super) fn clear(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_metadata = std::fs::symlink_metadata(parent)?;
    if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
        anyhow::bail!("hive outbox parent must be a prepared directory");
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            OpenOptions::new().write(true).truncate(true).open(path)?;
        }
        Ok(_) => anyhow::bail!("hive outbox must be a regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new().write(true).create_new(true).open(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn drain(path: &Path) -> anyhow::Result<Vec<Utterance>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        anyhow::bail!("hive outbox must be a prepared regular file")
    }
    if metadata.len() > MAX_OUTBOX_BYTES {
        anyhow::bail!("hive outbox exceeds {MAX_OUTBOX_BYTES}-byte limit")
    }
    Ok(std::fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}
