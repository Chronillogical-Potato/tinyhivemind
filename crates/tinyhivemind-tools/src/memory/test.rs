//! The memory tools: schemas, per-seat attribution, and refusals.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::*;
use serde_json::json;
use std::{io, sync::Mutex};
use tinyhivemind_core::runtime::{MemoryEntry, MemoryFuture, MemoryQuery};

#[derive(Default)]
struct Shelf {
    entries: Mutex<Vec<MemoryEntry>>,
    broken: bool,
}

impl WorkingMemory for Shelf {
    fn recall<'a>(&'a self, query: &'a MemoryQuery) -> MemoryFuture<'a, Vec<MemoryEntry>> {
        Box::pin(async move {
            if self.broken {
                return Err(Box::new(io::Error::other("secret path /x")) as _);
            }
            Ok(self
                .entries
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.scope == MemoryScope::Hive || e.author == query.seat)
                .cloned()
                .collect())
        })
    }
    fn record<'a>(&'a self, note: &'a MemoryNote) -> MemoryFuture<'a, MemoryEntry> {
        Box::pin(async move {
            if self.broken {
                return Err(Box::new(io::Error::other("secret path /x")) as _);
            }
            let mut held = self.entries.lock().unwrap();
            let entry = MemoryEntry {
                id: format!("m{}", held.len()),
                author: note.author.clone(),
                scope: note.scope,
                text: note.text.clone(),
            };
            held.push(entry.clone());
            Ok(entry)
        })
    }
    fn forget<'a>(&'a self, _: &'a str, id: &'a str) -> MemoryFuture<'a, ()> {
        Box::pin(async move {
            if self.broken {
                return Err(Box::new(io::Error::other("secret path /x")) as _);
            }
            self.entries.lock().unwrap().retain(|e| e.id != id);
            Ok(())
        })
    }
}

fn tools(broken: bool) -> MemoryTools {
    MemoryTools::new(Arc::new(Shelf {
        broken,
        ..Shelf::default()
    }))
}

#[test]
fn serves_three_tools_each_with_an_object_schema() {
    let defs = memory_tool_definitions();
    assert_eq!(defs.len(), 3);
    for (def, name) in defs.iter().zip(MEMORY_TOOLS) {
        assert_eq!(def.name, name);
        assert_eq!(def.parameters["type"], "object");
        assert!(MemoryTools::serves(name));
    }
    assert!(!MemoryTools::serves("post"));
}

#[tokio::test]
async fn a_note_is_attributed_to_the_calling_seat_and_recalled() {
    let t = tools(false);
    let said = t
        .call("solver", "hive_memory_note", &json!({"text": "use --no-cache"}))
        .await
        .unwrap();
    assert_eq!(said, "remembered as m0");
    let got = t
        .call("reviewer", "hive_memory_recall", &json!({}))
        .await
        .unwrap();
    assert_eq!(got, "m0 [solver] use --no-cache");
}

#[tokio::test]
async fn a_private_note_is_not_recalled_by_a_peer() {
    let t = tools(false);
    t.call(
        "solver",
        "hive_memory_note",
        &json!({"text": "hunch", "scope": "seat"}),
    )
    .await
    .unwrap();
    let peer = t
        .call("reviewer", "hive_memory_recall", &json!({}))
        .await
        .unwrap();
    assert_eq!(peer, "nothing remembered");
}

#[tokio::test]
async fn forget_drops_an_entry() {
    let t = tools(false);
    t.call("a", "hive_memory_note", &json!({"text": "stale"}))
        .await
        .unwrap();
    assert_eq!(
        t.call("a", "hive_memory_forget", &json!({"id": "m0"}))
            .await
            .unwrap(),
        "forgot m0"
    );
    assert_eq!(
        t.call("a", "hive_memory_recall", &json!({})).await.unwrap(),
        "nothing remembered"
    );
}

#[tokio::test]
async fn refuses_missing_and_malformed_arguments() {
    let t = tools(false);
    assert!(t.call("a", "hive_memory_note", &json!({})).await.is_err());
    assert!(t.call("a", "hive_memory_forget", &json!({})).await.is_err());
    let bad = t
        .call("a", "hive_memory_note", &json!({"text": "x", "scope": "galaxy"}))
        .await
        .unwrap_err();
    assert!(bad.contains("galaxy"));
    assert!(t.call("a", "memory_dance", &json!({})).await.is_err());
}

#[tokio::test]
async fn refuses_a_blank_or_oversized_note_in_the_ports_words() {
    let t = tools(false);
    let blank = t
        .call("a", "hive_memory_note", &json!({"text": " "}))
        .await
        .unwrap_err();
    assert_eq!(blank, "memory note is empty");
    let long = "x".repeat(MEMORY_NOTE_CHARS + 1);
    let over = t
        .call("a", "hive_memory_note", &json!({"text": long}))
        .await
        .unwrap_err();
    assert!(over.contains("limit"));
}

#[tokio::test]
async fn an_engine_failure_never_leaks_its_message_to_the_seat() {
    let t = tools(true);
    for (name, args) in [
        ("hive_memory_recall", json!({})),
        ("hive_memory_note", json!({"text": "x"})),
        ("hive_memory_forget", json!({"id": "m0"})),
    ] {
        let err = t.call("a", name, &args).await.unwrap_err();
        assert!(!err.contains("secret"), "{err}");
        assert!(err.contains("unavailable"));
    }
}
