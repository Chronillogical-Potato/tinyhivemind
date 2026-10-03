//! Framing and the refusal shape, without a socket.

#![allow(clippy::expect_used)]

use serde_json::json;

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{capabilities, content_length, headers_end, refusal, result, seat_for, serve_with};
use crate::EpisodeTools;

#[test]
fn the_head_ends_at_the_blank_line() {
    assert_eq!(
        headers_end(b"POST / HTTP/1.1\r\nA: b\r\n\r\nbody"),
        Some(25)
    );
    assert_eq!(headers_end(b"POST / HTTP/1.1\r\nA: b\r\n"), None);
}

#[test]
fn content_length_is_read_case_insensitively_and_defaults_to_zero() {
    assert_eq!(
        content_length("POST / HTTP/1.1\r\ncontent-LENGTH: 12\r\n"),
        12
    );
    assert_eq!(
        content_length("POST / HTTP/1.1\r\nContent-Length: nope\r\n"),
        0
    );
    assert_eq!(content_length("POST / HTTP/1.1\r\n"), 0);
}

#[test]
fn a_refusal_is_a_result_the_seat_reads_not_a_protocol_error() {
    let refused = refusal(&json!(7), "no");
    assert_eq!(refused["id"], 7);
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(refused["result"]["content"][0]["text"], "no");
    assert!(
        refused.get("error").is_none(),
        "a refusal never fails the call"
    );
    let ok = result(&json!(8), "yes");
    assert!(ok["result"].get("isError").is_none());
    assert_eq!(ok["result"]["content"][0]["text"], "yes");
}

#[test]
fn a_capability_is_per_seat_per_server_and_never_derivable_from_the_name() {
    let seats = ["lead".to_owned(), "solver".to_owned()];
    let first = capabilities(&seats);
    let second = capabilities(&seats);
    assert_eq!(first.len(), 2);
    for token in first.values() {
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }
    assert_ne!(first["lead"], first["solver"]);
    assert_ne!(
        first["lead"], second["lead"],
        "a new server mints new capabilities"
    );
}

#[test]
fn a_path_speaks_for_a_seat_only_with_that_seats_capability() {
    let seats = ["lead".to_owned()];
    let minted = capabilities(&seats);
    let good = format!("/seat/lead/{}", minted["lead"]);
    assert_eq!(seat_for(&good, &minted).as_deref(), Some("lead"));
    assert_eq!(seat_for("/seat/lead/", &minted), None);
    assert_eq!(seat_for("/seat/lead", &minted), None);
    assert_eq!(
        seat_for(&format!("/seat/solver/{}", minted["lead"]), &minted),
        None,
        "another seat's capability is not this seat's"
    );
    assert_eq!(seat_for("/nowhere", &minted), None);
}

#[tokio::test]
async fn a_started_request_that_never_finishes_is_closed_at_the_deadline() {
    let tools = Arc::new(EpisodeTools::new(["lead"]));
    let server = serve_with(Arc::clone(&tools), Duration::from_millis(200))
        .await
        .expect("loopback binds");
    let mut stream = TcpStream::connect(("127.0.0.1", server.port()))
        .await
        .expect("connects");
    stream
        .write_all(b"POST /seat/lead/x HTTP/1.1\r\nContent-Length: 5\r\n")
        .await
        .expect("writes a partial head");
    let mut sink = [0_u8; 16];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut sink))
        .await
        .expect("the server closes well before this")
        .expect("a close is not an error");
    assert_eq!(read, 0, "the connection is closed, not answered");
}
