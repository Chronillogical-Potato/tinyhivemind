//! The live wire, end to end over loopback.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::sync::Arc;

use serde_json::{Value, json};
use tinyhivemind::speech::{ToolCall, Utterance};
use tinyhivemind_mcp::{Dispatch, EpisodeTools, PROTOCOL_VERSION, Refusal, Server, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// One POST, one reply. The client the harness ships keeps a connection open
/// across calls; a fresh one per call is the same wire.
async fn post(port: u16, path: &str, body: Value) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let body = serde_json::to_vec(&body).unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();
    stream.flush().await.unwrap();
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read != 0, "connection closed before a reply");
        raw.extend_from_slice(&chunk[..read]);
        let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&raw[..end]).to_string();
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        if raw.len() >= end + 4 + length {
            let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
            let value = if length == 0 {
                Value::Null
            } else {
                serde_json::from_slice(&raw[end + 4..end + 4 + length]).unwrap()
            };
            return (status, value);
        }
    }
}

/// The path a seat dials: its endpoint without the origin.
fn path_of(server: &Server, seat: &str) -> String {
    let endpoint = server.endpoint(seat);
    let after_scheme = endpoint
        .strip_prefix("http://")
        .expect("endpoints are http");
    let slash = after_scheme.find('/').expect("endpoints have a path");
    after_scheme[slash..].to_owned()
}

fn rpc(id: u64, method: &str, params: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn call(name: &str, arguments: &Value) -> Value {
    rpc(
        1,
        "tools/call",
        &json!({ "name": name, "arguments": arguments }),
    )
}

fn text(reply: &Value) -> &str {
    reply["result"]["content"][0]["text"].as_str().unwrap()
}

fn dispatch() -> Dispatch {
    Dispatch {
        chat: "engineering".into(),
        parent: Some("42".into()),
    }
}

fn in_thread(extra: &Value) -> Value {
    let mut merged = json!({ "chat": "engineering", "parent": "42" });
    for (key, value) in extra.as_object().unwrap() {
        merged[key] = value.clone();
    }
    merged
}

fn desk_dispatch() -> Dispatch {
    Dispatch {
        chat: "engineering".into(),
        parent: None,
    }
}

fn on_desk(extra: &Value) -> Value {
    let mut merged = json!({ "chat": "engineering", "parent": null });
    for (key, value) in extra.as_object().unwrap() {
        merged[key] = value.clone();
    }
    merged
}

async fn stand_up() -> (Arc<EpisodeTools>, Server) {
    let tools = Arc::new(EpisodeTools::new(["lead", "solver", "checker"]));
    let server = serve(Arc::clone(&tools)).await.expect("loopback binds");
    (tools, server)
}

#[tokio::test]
async fn the_handshake_echoes_the_protocol_version_and_lists_the_served_tools() {
    let (_tools, server) = stand_up().await;
    let (status, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        rpc(1, "initialize", &json!({})),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);
    let (status, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(status, 202, "a notification takes no reply");
    assert_eq!(reply, Value::Null);
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        rpc(2, "tools/list", &json!({})),
    )
    .await;
    let names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["broadcast", "ask", "complete_episode", "read"]);
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        rpc(3, "nope", &json!({})),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32601);
    let endpoint = server.endpoint("lead");
    let prefix = format!("http://127.0.0.1:{}/seat/lead/", server.port());
    let capability = endpoint
        .strip_prefix(&prefix)
        .expect("the endpoint carries the seat's capability after its name");
    assert_eq!(capability.len(), 32);
    assert!(capability.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(
        server.endpoint("solver").rsplit('/').next().unwrap(),
        capability,
        "each seat holds its own"
    );
}

#[tokio::test]
async fn a_call_is_recorded_against_the_seat_that_dialled() {
    let (tools, server) = stand_up().await;
    tools.register("lead", dispatch());
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call(
            "complete_episode",
            &in_thread(&json!({ "message": "done" })),
        ),
    )
    .await;
    assert_eq!(text(&reply), "recorded: your assignment is complete");
    assert!(reply["result"].get("isError").is_none());
    let events = tools.drain("lead");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seat, "lead");
    assert_eq!(events[0].dispatch, dispatch());
    assert_eq!(
        events[0].call,
        ToolCall::Speak(Utterance::CompleteEpisode {
            message: "done".into()
        })
    );
    assert!(
        tools.drain("solver").is_empty(),
        "nothing lands on another seat"
    );
}

#[tokio::test]
async fn a_seat_with_no_open_turn_or_the_wrong_thread_is_refused() {
    let (tools, server) = stand_up().await;
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call("complete_episode", &in_thread(&json!({ "message": "hi" }))),
    )
    .await;
    assert_eq!(reply["result"]["isError"], true);
    assert!(text(&reply).contains("no turn is open"));

    tools.register("lead", dispatch());
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call(
            "complete_episode",
            &json!({ "message": "hi", "chat": "marketing", "parent": "42" }),
        ),
    )
    .await;
    assert_eq!(reply["result"]["isError"], true);
    assert!(
        text(&reply).contains("chat `engineering` with parent `42`"),
        "{}",
        text(&reply)
    );
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call(
            "complete_episode",
            &json!({ "message": "hi", "chat": "engineering" }),
        ),
    )
    .await;
    assert_eq!(
        reply["result"]["isError"], true,
        "a missing parent is a mismatch too"
    );
    assert!(
        tools.drain("lead").is_empty(),
        "nothing refused is recorded"
    );

    let (_, reply) = post(
        server.port(),
        &path_of(&server, "johnny"),
        call("complete_episode", &in_thread(&json!({ "message": "hi" }))),
    )
    .await;
    assert_eq!(
        reply["error"]["message"], "unknown endpoint",
        "a seat the server does not serve has no capability, so no endpoint"
    );
}

#[tokio::test]
async fn refusals_are_the_vocabularys_own_sentences() {
    let (tools, server) = stand_up().await;
    tools.register("lead", desk_dispatch());
    let port = server.port();
    let (_, reply) = post(
        port,
        &path_of(&server, "lead"),
        call("complete_episode", &on_desk(&json!({ "message": "  " }))),
    )
    .await;
    assert_eq!(text(&reply), "`message` must be a non-empty string");
    let (_, reply) = post(
        port,
        &path_of(&server, "lead"),
        call("dm", &on_desk(&json!({ "message": "x", "to": ["solver"] }))),
    )
    .await;
    assert_eq!(
        text(&reply),
        "unknown tool dm",
        "dm is in the vocabulary and not served"
    );
    let (_, reply) = post(
        port,
        &path_of(&server, "lead"),
        call(
            "ask",
            &on_desk(&json!({ "message": "?", "to": ["solver", "checker"] })),
        ),
    )
    .await;
    assert_eq!(
        text(&reply),
        "`to` must name exactly one seat to ask; 2 were named"
    );
    let (_, reply) = post(
        port,
        &path_of(&server, "lead"),
        call("ask", &on_desk(&json!({ "message": "?", "to": "lead" }))),
    )
    .await;
    assert_eq!(
        text(&reply),
        "`to` names you; a message to yourself reaches nobody else"
    );
    let (_, reply) = post(
        port,
        &path_of(&server, "lead"),
        call("ask", &on_desk(&json!({ "message": "?", "to": "johnny" }))),
    )
    .await;
    assert_eq!(
        text(&reply),
        "`to` names @johnny, who is not an active seat on this desk. The desk is: checker, lead, solver"
    );
    assert!(tools.drain("lead").is_empty());
}

#[tokio::test]
async fn an_ask_is_recorded_and_told_the_answer_comes_later() {
    let (tools, server) = stand_up().await;
    tools.register("lead", desk_dispatch());
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call(
            "ask",
            &on_desk(&json!({ "message": "is it tight?", "to": "solver" })),
        ),
    )
    .await;
    assert!(text(&reply).starts_with("asked @solver."));
    assert!(text(&reply).contains("later turn"));
    let events = tools.drain("lead");
    assert_eq!(
        events[0].call,
        ToolCall::Speak(Utterance::Ask {
            to: "solver".into(),
            message: "is it tight?".into()
        })
    );
}

#[tokio::test]
async fn read_returns_the_window_the_host_refreshed_and_records_nothing() {
    let (tools, server) = stand_up().await;
    tools.register(
        "lead",
        Dispatch {
            chat: "engineering".into(),
            parent: None,
        },
    );
    tools.window("lead", vec!["one".into(), "two".into(), "three".into()]);
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call(
            "read",
            &json!({ "limit": 2, "chat": "engineering", "parent": null }),
        ),
    )
    .await;
    assert_eq!(text(&reply), "two\nthree");
    assert!(
        tools.drain("lead").is_empty(),
        "a read is answered, not recorded"
    );
}

#[tokio::test]
async fn shutdown_stops_accepting() {
    let (_tools, server) = stand_up().await;
    let port = server.port();
    server.shutdown();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
}

#[tokio::test]
async fn an_ask_inside_a_conversation_is_refused_with_what_to_do_instead() {
    let (tools, server) = stand_up().await;
    tools.register("solver", dispatch());
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "solver"),
        call("ask", &in_thread(&json!({ "message": "?", "to": "lead" }))),
    )
    .await;
    assert_eq!(reply["result"]["isError"], true);
    assert!(text(&reply).starts_with("inside a conversation you answer the seat that asked you"));
    assert!(tools.drain("solver").is_empty());
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "solver"),
        call(
            "broadcast",
            &in_thread(&json!({ "message": "work for someone" })),
        ),
    )
    .await;
    assert!(
        reply["result"].get("isError").is_none(),
        "a broadcast from a thread is served"
    );
}

#[tokio::test]
async fn a_refusal_is_drained_by_the_host_with_the_sentence_the_seat_read() {
    let (tools, server) = stand_up().await;
    tools.register("solver", dispatch());
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "solver"),
        call("ask", &in_thread(&json!({ "message": "?", "to": "lead" }))),
    )
    .await;
    assert_eq!(reply["result"]["isError"], true);
    let refused = tools.drain_refusals("solver");
    assert_eq!(
        refused,
        vec![Refusal {
            seat: "solver".into(),
            tool: "ask".into(),
            reason: text(&reply).to_owned(),
        }]
    );
    assert!(tools.drain_refusals("solver").is_empty(), "drained once");
    assert!(
        tools.drain("solver").is_empty(),
        "a refusal records no call"
    );
}

#[tokio::test]
async fn an_endpoint_is_a_capability_and_a_wrong_one_learns_nothing() {
    let (tools, server) = stand_up().await;
    tools.register("lead", dispatch());
    // The seat's name with someone else's capability, or none at all.
    for path in [
        format!(
            "/seat/lead/{}",
            path_of(&server, "solver").rsplit('/').next().unwrap()
        ),
        "/seat/lead/".to_owned(),
        "/seat/lead".to_owned(),
    ] {
        let (status, reply) = post(server.port(), &path, rpc(1, "tools/list", &json!({}))).await;
        assert_eq!(status, 200);
        assert_eq!(reply["error"]["message"], "unknown endpoint");
        assert!(
            reply.get("result").is_none(),
            "no tools are listed at {path}"
        );
        let (_, reply) = post(
            server.port(),
            &path,
            call("complete_episode", &on_desk(&json!({ "message": "hi" }))),
        )
        .await;
        assert_eq!(reply["error"]["message"], "unknown endpoint");
        let shown = reply.to_string();
        assert!(
            !shown.contains("engineering") && !shown.contains("42"),
            "the turn lead is in is not disclosed: {shown}"
        );
    }
    assert!(tools.drain("lead").is_empty());
    assert!(
        tools.drain_refusals("lead").is_empty(),
        "nothing reached the seat"
    );
    // With the capability, the same call is served.
    let (_, reply) = post(
        server.port(),
        &path_of(&server, "lead"),
        call("complete_episode", &in_thread(&json!({ "message": "hi" }))),
    )
    .await;
    assert!(reply["result"].get("isError").is_none());
    assert_eq!(tools.drain("lead").len(), 1);
}

/// Write raw bytes and report whether the server closed the connection.
async fn is_closed_after(port: u16, bytes: &[u8]) -> bool {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(bytes).await.unwrap();
    stream.flush().await.unwrap();
    let mut sink = [0_u8; 64];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut sink))
        .await
        .expect("the server answers or closes")
        .unwrap_or(0);
    read == 0
}

#[tokio::test]
async fn an_oversized_head_or_body_closes_the_connection() {
    let (_tools, server) = stand_up().await;
    let path = path_of(&server, "lead");
    let mut long_head = format!("POST {path} HTTP/1.1\r\nX-Pad: ");
    long_head.push_str(&"a".repeat(9 * 1024));
    assert!(is_closed_after(server.port(), long_head.as_bytes()).await);
    let huge_body = format!("POST {path} HTTP/1.1\r\nContent-Length: 2000000\r\n\r\n");
    assert!(is_closed_after(server.port(), huge_body.as_bytes()).await);
    // A well-formed request on a fresh connection is still served.
    let (status, _) = post(server.port(), &path, rpc(1, "initialize", &json!({}))).await;
    assert_eq!(status, 200);
}
