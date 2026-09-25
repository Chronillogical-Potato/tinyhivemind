//! The loopback listener and the four JSON-RPC methods MCP needs.
//!
//! Hand-rolled over `tokio::net` rather than a web framework: the surface is
//! one POST that takes JSON and returns JSON, on loopback, from a client the
//! host configured. The framing is the smallest HTTP/1.1 that client speaks.
//!
//! **The seat is the endpoint it dialled.** `/seat/<id>` is the whole of a
//! caller's identity; nothing in a payload can change who is calling.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use crate::{Error, Result};
use tinyhivemind_tools::{EpisodeTools, raw_arguments};

/// The MCP protocol version negotiated. Echoed exactly, or the client refuses.
pub const PROTOCOL_VERSION: &str = "2025-11-25";

/// A started request must finish within this; an idle connection may wait.
const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
/// The most a request head may run to before the connection is closed.
const MAX_HEAD_BYTES: usize = 8 * 1024;
/// The most a body may declare before the connection is closed.
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// A running server: the port a client dials, and the means to stop it.
#[derive(Debug)]
pub struct Server {
    port: u16,
    stop: Option<oneshot::Sender<()>>,
    /// One capability per served seat, minted when the server bound.
    capabilities: Arc<BTreeMap<String, String>>,
}

impl Server {
    /// The loopback port the server is listening on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The endpoint one seat is given:
    /// `http://127.0.0.1:<port>/seat/<id>/<capability>`.
    ///
    /// The capability is minted when the server binds and is what makes the
    /// endpoint an identity rather than a label: a process that can reach
    /// loopback and read `tools/list` still cannot speak as a seat it was not
    /// handed. A seat this server does not serve gets an endpoint it refuses.
    #[must_use]
    pub fn endpoint(&self, seat: &str) -> String {
        format!(
            "http://127.0.0.1:{}/seat/{seat}/{}",
            self.port,
            self.capabilities.get(seat).map_or("", String::as_str)
        )
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
    serve_with(tools, REQUEST_DEADLINE).await
}

/// [`serve`], with the deadline a started request must finish within.
pub(crate) async fn serve_with(tools: Arc<EpisodeTools>, deadline: Duration) -> Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(Error::Bind)?;
    let port = listener.local_addr().map_err(Error::Bind)?.port();
    let capabilities = Arc::new(capabilities(&tools.seats()));
    let served = Arc::clone(&capabilities);
    let (stop, mut stopped) = oneshot::channel();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stopped => return,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let tools = Arc::clone(&tools);
                    let served = Arc::clone(&served);
                    tokio::spawn(async move {
                        let _ = session(stream, tools, served, deadline).await;
                    });
                }
            }
        }
    });
    Ok(Server {
        port,
        stop: Some(stop),
        capabilities,
    })
}

/// One unguessable capability per seat.
///
/// The keys behind `RandomState` are drawn from the operating system, so a
/// token cannot be derived from a seat's name, from another seat's token, or
/// from a previous server's. Two lanes make it 128 bits wide.
fn capabilities(seats: &[String]) -> BTreeMap<String, String> {
    let salt = RandomState::new();
    seats
        .iter()
        .map(|seat| {
            let mut token = String::with_capacity(32);
            for lane in 0_u8..2 {
                let _ = write!(token, "{:016x}", salt.hash_one((lane, seat.as_str())));
            }
            (seat.clone(), token)
        })
        .collect()
}

/// The seat a path speaks for, if it carries that seat's capability.
fn seat_for(path: &str, capabilities: &BTreeMap<String, String>) -> Option<String> {
    let rest = path.strip_prefix("/seat/")?;
    let (seat, token) = rest.split_once('/')?;
    (capabilities.get(seat)? == token).then(|| seat.to_owned())
}

/// One connection. The client keeps it alive across calls, so this loops.
async fn session(
    mut stream: TcpStream,
    tools: Arc<EpisodeTools>,
    capabilities: Arc<BTreeMap<String, String>>,
    deadline: Duration,
) -> std::io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        let Some((path, body, consumed)) = read_request(&mut stream, &mut buffer, deadline).await?
        else {
            return Ok(());
        };
        buffer.drain(..consumed);
        let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        // Without the seat's capability there is no seat, and nothing to
        // learn: not the tools, not the turn a seat is in.
        let Some(seat) = seat_for(&path, &capabilities) else {
            let response = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32001, "message": "unknown endpoint" },
            });
            write_response(&mut stream, Some(&response)).await?;
            continue;
        };
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
                "result": { "tools": tools.tool_definitions() },
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

/// One `tools/call` from an authenticated seat: the framing, over
/// [`EpisodeTools::call`].
///
/// The seat is the capability its endpoint carried; the name and the
/// arguments are the request's. Everything that decides -- turn, thread,
/// `interpret`, the record and the refusal copy -- is the in-process call, so
/// a seat reached over this wire and a seat handed the tools natively are
/// refused and acknowledged alike.
fn call(tools: &EpisodeTools, seat: &str, request: &Value, id: &Value) -> Value {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match tools.call(seat, name, &raw_arguments(&params)) {
        Ok(text) => result(id, &text),
        Err(text) => refusal(id, &text),
    }
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
    deadline: Duration,
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
            let length = content_length(&head);
            if length > MAX_BODY_BYTES {
                return Ok(None);
            }
            let total = head_end + length;
            if buffer.len() >= total {
                return Ok(Some((path, buffer[head_end..total].to_vec(), total)));
            }
        } else if buffer.len() > MAX_HEAD_BYTES {
            return Ok(None);
        }
        let mut chunk = [0_u8; 4096];
        // An idle connection may wait as long as it likes; a request that has
        // started must finish within the deadline, or the connection closes.
        let read = if buffer.is_empty() {
            stream.read(&mut chunk).await?
        } else {
            match tokio::time::timeout(deadline, stream.read(&mut chunk)).await {
                Ok(read) => read?,
                Err(_) => return Ok(None),
            }
        };
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
