//! Policy, truncation and RPC framing, with the peer played in-process.

use std::io::{BufReader, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use super::rpc::decode_reply;
use super::*;

#[test]
fn refuses_empty_and_destructive_commands() {
    assert_eq!(refuse("   "), Some("empty command"));
    assert!(refuse("rm -rf /").is_some());
    assert!(refuse("echo hi; rm  -rf  / ").is_some());
    assert!(refuse(":(){ :|:& };:").is_some());
    assert!(refuse("rm -rf ./build").is_none());
    assert!(refuse("pytest -x tests/").is_none());
    assert!(refuse(&"x".repeat(MAX_COMMAND_CHARS + 1)).is_some());
}

#[test]
fn truncate_keeps_head_and_tail() {
    let text = format!("{}{}", "a".repeat(100), "z".repeat(100));
    let cut = truncate(&text, 40);
    assert!(cut.starts_with(&"a".repeat(20)));
    assert!(cut.ends_with(&"z".repeat(20)));
    assert!(cut.contains("160 bytes truncated"));
    assert_eq!(truncate("short", 40), "short");
}

#[test]
fn truncate_respects_char_boundaries() {
    let text = "é".repeat(50);
    let cut = truncate(&text, 31);
    assert!(cut.contains("truncated"));
}

#[test]
fn request_lines_are_json_with_id_exec_timeout() {
    let line = encode_request(7, "ls \"x\"", Duration::from_secs(30));
    let value: serde_json::Value = serde_json::from_str(&line).expect("json");
    assert_eq!(value["id"], 7);
    assert_eq!(value["exec"], "ls \"x\"");
    assert_eq!(value["timeout"], 30);
}

#[test]
fn replies_decode_and_garbage_is_ignored() {
    let reply = decode_reply(r#"{"id":3,"stdout":"ok","exit":2}"#).expect("reply");
    assert_eq!((reply.id, reply.stdout.as_str(), reply.exit), (3, "ok", 2));
    assert!(decode_reply("not json").is_none());
    assert!(decode_reply(r#"{"stdout":"no id"}"#).is_none());
}

struct ChannelReader(mpsc::Receiver<Vec<u8>>, Vec<u8>);

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(bytes) => self.1 = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.1.len());
        buf[..n].copy_from_slice(&self.1[..n]);
        self.1.drain(..n);
        Ok(n)
    }
}

struct ChannelWriter(mpsc::Sender<Vec<u8>>);

impl Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A peer that answers every request with `ran:<cmd>`, replies out of order
/// when asked, and closes when `close` is set.
fn peer(close_after: Option<usize>) -> StdioExec {
    let (req_tx, req_rx) = mpsc::channel::<Vec<u8>>();
    let (rep_tx, rep_rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut seen = 0;
        for line in std::io::BufRead::lines(BufReader::new(ChannelReader(req_rx, Vec::new()))) {
            let Ok(line) = line else { break };
            let value: serde_json::Value = serde_json::from_str(&line).expect("request json");
            let reply = serde_json::json!({
                "id": value["id"], "stdout": format!("ran:{}", value["exec"].as_str().unwrap_or("")), "exit": 0
            });
            let _ = rep_tx.send(format!("{reply}\n").into_bytes());
            seen += 1;
            if close_after == Some(seen) {
                break;
            }
        }
    });
    StdioExec::new(
        BufReader::new(ChannelReader(rep_rx, Vec::new())),
        ChannelWriter(req_tx),
    )
}

#[test]
fn concurrent_callers_get_their_own_replies() {
    let exec = std::sync::Arc::new(peer(None));
    let handles: Vec<_> = (0..6)
        .map(|n| {
            let exec = std::sync::Arc::clone(&exec);
            std::thread::spawn(move || exec.exec(&format!("cmd{n}"), Duration::from_secs(5)))
        })
        .collect();
    for (n, handle) in handles.into_iter().enumerate() {
        let out = handle.join().expect("thread").expect("reply");
        assert_eq!(out.stdout, format!("ran:cmd{n}"));
    }
}

#[test]
fn a_closed_peer_fails_the_call_instead_of_hanging() {
    let exec = peer(Some(1));
    assert!(exec.exec("one", Duration::from_secs(1)).is_ok());
    assert!(exec.exec("two", Duration::from_secs(1)).is_err());
}
