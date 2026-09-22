//! The loopback listener and the four JSON-RPC methods MCP needs.
//!
//! Hand-rolled over `tokio::net` rather than a web framework: the surface is
//! one POST that takes JSON and returns JSON, on loopback, from a client the
//! host configured. The framing is the smallest HTTP/1.1 that client speaks.
//!
//! **The seat is the endpoint it dialled.** `/seat/<id>` is the whole of a
//! caller's identity; nothing in a payload can change who is calling.

use std::sync::Arc;

use serde_json::{Value, json};
use tinyhivemind::speech::{ToolCall, Utterance, UtteranceRejection, interpret};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::render::{arguments, serves, tool_definitions};
use crate::tools::{EpisodeTools, SeatEvent};
use crate::{Error, Result};

/// The MCP protocol version negotiated. Echoed exactly, or the client refuses.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// A running server: the port a client dials, and the means to stop it.
#[derive(Debug)]
pub struct Server {
    port: u16,
    stop: Option<oneshot::Sender<()>>,
}

impl Server {
    /// The loopback port the server is listening on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The endpoint one seat is given: `http://127.0.0.1:<port>/seat/<id>`.
    #[must_use]
    pub fn endpoint(&self, seat: &str) -> String {
        format!("http://127.0.0.1:{}/seat/{seat}", self.port)
    }

    /// Stop accepting connections. Open sessions finish their current call.
    pub fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Bind loopback on a port the OS picks and serve until shut down or dropped.
///
/// # Errors
///
/// Returns [`Error::Bind`] when loopback cannot be bound.
pub async fn serve(tools: Arc<EpisodeTools>) -> Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(Error::Bind)?;
    let port = listener.local_addr().map_err(Error::Bind)?.port();
    let (stop, mut stopped) = oneshot::channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stopped => return,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let tools = Arc::clone(&tools);
                    tokio::spawn(async move {
                        let _ = session(stream, tools).await;
                    });
                }
            }
        }
    });
    Ok(Server {
        port,
        stop: Some(stop),
    })
}

/// One connection. The client keeps it alive across calls, so this loops.
async fn session(mut stream: TcpStream, tools: Arc<EpisodeTools>) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        let Some((path, body, consumed)) = read_request(&mut stream, &mut buffer).await? else {
            return Ok(());
        };
        buffer.drain(..consumed);
        let seat = path.rsplit('/').next().unwrap_or_default().to_owned();
        let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let response = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "tinyhivemind-episode",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                },
            })),
            // A notification has no id and takes no reply.
            "notifications/initialized" => None,
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": tool_definitions(&tools.seats()) },
            })),
            "tools/call" => Some(call(&tools, &seat, &request, &id)),
            other => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("unknown method {other}") },
            })),
        };
        write_response(&mut stream, response.as_ref()).await?;
    }
}

/// One `tools/call`: check the caller, check the turn, read the call, record it.
fn call(tools: &EpisodeTools, seat: &str, request: &Value, id: &Value) -> Value {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let args = arguments(&params);

    if !tools.knows(seat) {
        return refusal(id, &format!("no seat named `{seat}` is served here"));
    }
    let Some(dispatch) = tools.open_turn(seat) else {
        return refusal(
            id,
            "no turn is open for you, so nothing you call now can be recorded",
        );
    };
    // The seat says which turn it thinks it is in; the host said which turn
    // it is running. A mismatch is a confused model, and it is told so
    // rather than having its call land on the wrong episode.
    if args.chat.as_deref() != Some(dispatch.chat.as_str()) || args.parent != dispatch.parent {
        let parent = dispatch
            .parent
            .as_deref()
            .map_or_else(|| "null".to_owned(), |parent| format!("`{parent}`"));
        return refusal(
            id,
            &format!(
                "this turn is in chat `{}` with parent {parent}; name exactly those",
                dispatch.chat
            ),
        );
    }
    if !serves(name) {
        return refusal(id, &unknown_tool(name));
    }
    let call = match interpret(name, &args.call()) {
        Ok(call) => call,
        Err(rejection) => return refusal(id, &rejection.to_string()),
    };
    let acknowledgement = match &call {
        ToolCall::Read { limit } => {
            let rows = tools.recent(seat, *limit);
            return result(id, &rows.join("\n"));
        }
        ToolCall::Speak(Utterance::Ask { to, .. }) => {
            if to == seat {
                return refusal(id, &UtteranceRejection::SelfRecipient.to_string());
            }
            if !tools.knows(to) {
                return refusal(
                    id,
                    &format!(
                        "{}. The desk is: {}",
                        UtteranceRejection::UnknownRecipient { id: to.clone() },
                        tools.seats().join(", ")
                    ),
                );
            }
            format!(
                "asked @{to}. The answer reaches you on a later turn; you cannot finish \
                 until it does, so end your turn when you have asked everything."
            )
        }
        ToolCall::Speak(Utterance::Post { .. }) => "posted to the desk".to_owned(),
        ToolCall::Speak(Utterance::Broadcast { .. }) => {
            "recorded: routing will place that with a seat".to_owned()
        }
        ToolCall::Speak(Utterance::CompleteEpisode { .. }) => {
            "recorded: your assignment is complete".to_owned()
        }
        // Withheld, and refused above by name; kept exhaustive so a new
        // variant is a compile error here rather than a silent acceptance.
        ToolCall::Speak(Utterance::Dm { .. }) => return refusal(id, &unknown_tool(name)),
    };
    tools.record(SeatEvent {
        seat: seat.to_owned(),
        call,
        dispatch,
    });
    result(id, &acknowledgement)
}

fn unknown_tool(name: &str) -> String {
    UtteranceRejection::UnknownTool {
        name: name.to_owned(),
    }
    .to_string()
}

fn result(id: &Value, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "content": [{ "type": "text", "text": text }] },
    })
}

/// A refusal the **seat** can read, inside its own turn, while it can still
/// call again.
fn refusal(id: &Value, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "isError": true, "content": [{ "type": "text", "text": text }] },
    })
}

/// Read one HTTP/1.1 request: its path, body, and the bytes consumed.
///
/// `Ok(None)` is a closed connection rather than a failure: the client hangs
/// up when the episode ends.
async fn read_request(
    stream: &mut TcpStream,
    buffer: &mut Vec<u8>,
) -> std::io::Result<Option<(String, Vec<u8>, usize)>> {
    loop {
        if let Some(head_end) = headers_end(buffer) {
            let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
            let path = head
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_owned();
            let total = head_end + content_length(&head);
            if buffer.len() >= total {
                return Ok(Some((path, buffer[head_end..total].to_vec(), total)));
            }
        }
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn headers_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4)
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().ok())?
        })
        .unwrap_or(0)
}

async fn write_response(stream: &mut TcpStream, response: Option<&Value>) -> std::io::Result<()> {
    let Some(value) = response else {
        // A notification is acknowledged with no content, which is what the
        // client expects for `notifications/initialized`.
        stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n")
            .await?;
        return stream.flush().await;
    };
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: episode\r\n\
         MCP-Protocol-Version: {PROTOCOL_VERSION}\r\nContent-Length: {}\r\n\r\n",
        bytes.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await
}

#[cfg(test)]
mod test;
