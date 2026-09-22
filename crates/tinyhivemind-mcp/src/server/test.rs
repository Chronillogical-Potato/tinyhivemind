//! Framing and the refusal shape, without a socket.

#![allow(clippy::expect_used)]

use serde_json::json;

use super::{content_length, headers_end, refusal, result};

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
