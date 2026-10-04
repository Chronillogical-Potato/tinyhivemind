//! Unit tests for the working-memory port and its bounded rendering.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use std::{io, sync::Mutex};

/// An engine that keeps notes in order and ignores the limit it is given.
#[derive(Default)]
struct Shelf {
    entries: Mutex<Vec<MemoryEntry>>,
    broken: bool,
}

impl WorkingMemory for Shelf {
    fn recall<'a>(&'a self, query: &'a MemoryQuery) -> MemoryFuture<'a, Vec<MemoryEntry>> {
        Box::pin(async move {
            if self.broken {
                return Err(Box::new(io::Error::other("offline")) as BoxError);
            }
            let seen = self.entries.lock().expect("entries lock");
            Ok(seen
                .iter()
                .filter(|entry| entry.scope == MemoryScope::Hive || entry.author == query.seat)
                .cloned()
                .collect())
        })
    }

    fn record<'a>(&'a self, note: &'a MemoryNote) -> MemoryFuture<'a, MemoryEntry> {
        Box::pin(async move {
            if self.broken {
                return Err(Box::new(io::Error::other("offline")) as BoxError);
            }
            let mut held = self.entries.lock().expect("entries lock");
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
            self.entries
                .lock()
                .expect("entries lock")
                .retain(|entry| entry.id != id);
            Ok(())
        })
    }
}

fn note(author: &str, scope: MemoryScope, text: &str) -> MemoryNote {
    MemoryNote {
        author: author.into(),
        scope,
        text: text.into(),
    }
}

fn entry(author: &str, text: &str) -> MemoryEntry {
    MemoryEntry {
        id: "x".into(),
        author: author.into(),
        scope: MemoryScope::Hive,
        text: text.into(),
    }
}

#[tokio::test]
async fn a_recorded_note_is_recalled_by_its_peers() {
    let shelf = Shelf::default();
    record(&shelf, &note("solver", MemoryScope::Hive, "use --no-cache"))
        .await
        .expect("recorded");
    let found = recall(&shelf, "reviewer", "cache", 5)
        .await
        .expect("recalled");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "use --no-cache");
}

#[tokio::test]
async fn a_seat_private_note_stays_with_its_author() {
    let shelf = Shelf::default();
    record(&shelf, &note("solver", MemoryScope::Seat, "my hunch"))
        .await
        .unwrap();
    assert!(recall(&shelf, "reviewer", "", 5).await.unwrap().is_empty());
    assert_eq!(recall(&shelf, "solver", "", 5).await.unwrap().len(), 1);
}

#[tokio::test]
async fn recall_cuts_an_engine_that_ignores_the_limit() {
    let shelf = Shelf::default();
    for n in 0..30 {
        record(&shelf, &note("a", MemoryScope::Hive, &format!("fact {n}")))
            .await
            .unwrap();
    }
    assert_eq!(recall(&shelf, "a", "", 3).await.unwrap().len(), 3);
    assert_eq!(recall(&shelf, "a", "", 0).await.unwrap().len(), 1);
    assert_eq!(
        recall(&shelf, "a", "", 999).await.unwrap().len(),
        MEMORY_LIMIT
    );
}

#[tokio::test]
async fn forget_removes_an_entry() {
    let shelf = Shelf::default();
    let made = record(&shelf, &note("a", MemoryScope::Hive, "stale"))
        .await
        .unwrap();
    shelf.forget("a", &made.id).await.unwrap();
    assert!(recall(&shelf, "a", "", 5).await.unwrap().is_empty());
}

#[tokio::test]
async fn rejects_an_empty_note_before_the_engine_sees_it() {
    let shelf = Shelf::default();
    let err = record(&shelf, &note("a", MemoryScope::Hive, "  \n"))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::MemoryNoteEmpty));
    assert!(shelf.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_a_note_past_the_character_limit() {
    let shelf = Shelf::default();
    let long = "x".repeat(MEMORY_NOTE_CHARS + 1);
    let err = record(&shelf, &note("a", MemoryScope::Hive, &long))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::MemoryNoteTooLong { limit, actual }
        if limit == MEMORY_NOTE_CHARS && actual == MEMORY_NOTE_CHARS + 1));
    let at_limit = "x".repeat(MEMORY_NOTE_CHARS);
    assert!(validate_note(&note("a", MemoryScope::Hive, &at_limit)).is_ok());
}

#[tokio::test]
async fn an_engine_failure_is_a_typed_memory_error() {
    let shelf = Shelf {
        broken: true,
        ..Shelf::default()
    };
    let read = recall(&shelf, "a", "", 3).await.unwrap_err();
    let write = record(&shelf, &note("a", MemoryScope::Hive, "x"))
        .await
        .unwrap_err();
    assert!(matches!(read, Error::Memory { .. }));
    assert!(matches!(write, Error::Memory { .. }));
}

#[test]
fn renders_entries_in_the_engines_order() {
    let note = memory_note(&[entry("a", "first"), entry("b", "second")], 100).unwrap();
    assert_eq!(note.lines, ["[a] first", "[b] second"]);
}

#[test]
fn collapses_whitespace_so_an_entry_is_one_line() {
    let note = memory_note(&[entry("a", "one\n  two\tthree")], 100).unwrap();
    assert_eq!(note.lines, ["[a] one two three"]);
}

#[test]
fn stops_before_the_entry_that_passes_the_budget() {
    let entries = [
        entry("a", "short"),
        entry("b", &"y".repeat(200)),
        entry("c", "tail"),
    ];
    let note = memory_note(&entries, 50).unwrap();
    assert_eq!(note.lines, ["[a] short"]);
}

#[test]
fn renders_nothing_when_there_is_nothing_or_no_room() {
    assert!(memory_note(&[], 100).is_none());
    assert!(memory_note(&[entry("a", "does not fit")], 3).is_none());
}

#[test]
fn never_renders_more_than_the_entry_limit() {
    let entries: Vec<_> = (0..40).map(|n| entry("a", &n.to_string())).collect();
    assert_eq!(
        memory_note(&entries, 10_000).unwrap().lines.len(),
        MEMORY_LIMIT
    );
}

#[test]
fn pins_its_wire_form() {
    let note = note("solver", MemoryScope::Seat, "hi");
    assert_eq!(
        serde_json::to_value(&note).unwrap(),
        serde_json::json!({"author": "solver", "scope": "seat", "text": "hi"})
    );
    let back: MemoryNote =
        serde_json::from_value(serde_json::json!({"author": "a", "text": "t"})).unwrap();
    assert_eq!(back.scope, MemoryScope::Hive);
}
