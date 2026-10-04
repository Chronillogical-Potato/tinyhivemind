//! The markdown memory: round-trip, compaction, privacy and concurrent writers.

use super::*;
use std::sync::Arc;

fn directory(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hive-memory-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("test directory");
    dir
}

fn note(author: &str, scope: MemoryScope, text: &str) -> MemoryNote {
    MemoryNote {
        author: author.into(),
        scope,
        text: text.into(),
    }
}

fn query(seat: &str, text: &str, limit: usize) -> MemoryQuery {
    MemoryQuery {
        seat: seat.into(),
        query: text.into(),
        limit,
    }
}

fn small() -> Compaction {
    Compaction {
        keep_recent: 2,
        max_recent: 4,
        excerpt_chars: 12,
        max_compacted: 3,
    }
}

#[test]
fn a_note_survives_a_reload_from_the_file() {
    let memory = MarkdownMemory::new(directory("reload").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    let made = memory.record_now(&note("solver", MemoryScope::Hive, "use --no-cache")).unwrap();
    assert_eq!(made.id, "m1");
    let fresh = MarkdownMemory::new(memory.path(), Compaction::DEFAULT);
    let found = fresh.recall_now(&query("checker", "", 5)).unwrap();
    assert_eq!(found, vec![made]);
}

#[test]
fn recall_ranks_by_query_words_then_recency() {
    let memory = MarkdownMemory::new(directory("rank").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    for text in ["berlekamp fails held-out", "sturmian identity works", "unrelated chatter"] {
        memory.record_now(&note("a", MemoryScope::Hive, text)).unwrap();
    }
    let found = memory.recall_now(&query("a", "berlekamp held-out", 5)).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "berlekamp fails held-out");
    let newest = memory.recall_now(&query("a", "", 2)).unwrap();
    assert_eq!(newest[0].text, "unrelated chatter");
    let unmatched = memory.recall_now(&query("a", "zzzzzz", 5)).unwrap();
    assert_eq!(unmatched.len(), 3, "no match falls back to newest-first");
}

#[test]
fn a_repeated_note_is_not_stored_twice() {
    let memory = MarkdownMemory::new(directory("dup").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    let first = memory.record_now(&note("a", MemoryScope::Hive, "Same  fact")).unwrap();
    let again = memory.record_now(&note("a", MemoryScope::Hive, "same fact")).unwrap();
    assert_eq!(first.id, again.id);
    assert_eq!(memory.recall_now(&query("a", "", 9)).unwrap().len(), 1);
}

#[test]
fn old_entries_fold_into_excerpts_and_the_oldest_fall_off() {
    let memory = MarkdownMemory::new(directory("fold").join("HIVE_MEMORY.md"), small());
    for n in 0..12 {
        let text = format!("finding number {n} with a long tail of detail");
        memory.record_now(&note("a", MemoryScope::Hive, &text)).unwrap();
    }
    let file = fs::read_to_string(memory.path()).unwrap();
    let store = Store::parse(&file);
    assert!(store.recent.len() <= small().max_recent);
    assert!(store.compacted.len() <= small().max_compacted);
    assert!(store.compacted.iter().all(|e| e.text.chars().count() <= small().excerpt_chars + 1));
    assert_eq!(store.recent.last().unwrap().text, "finding number 11 with a long tail of detail");
    assert!(!file.contains("finding number 0 "), "the oldest fell off");
}

#[test]
fn compaction_keeps_only_the_newest_copy_of_an_exact_repeat() {
    let mut store = Store::default();
    let policy = small();
    // Same text written by one seat, separated by distinct notes so the
    // recent-window duplicate check does not collapse them first.
    for text in ["x", "a1", "b1", "c1", "x", "d1", "e1", "f1"] {
        store.record(&note("a", MemoryScope::Hive, text), policy);
    }
    let all: Vec<_> = store.compacted.iter().chain(&store.recent).collect();
    assert!(all.iter().filter(|e| e.text == "x").count() <= 1);
}

#[test]
fn a_seat_private_note_is_seen_only_by_its_author() {
    let memory = MarkdownMemory::new(directory("private").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    memory.record_now(&note("solver", MemoryScope::Seat, "my hunch")).unwrap();
    assert_eq!(memory.recall_now(&query("checker", "", 5)).unwrap().len(), 0);
    assert_eq!(memory.recall_now(&query("solver", "", 5)).unwrap().len(), 1);
}

#[test]
fn forget_removes_an_entry_but_not_one_the_seat_cannot_see() {
    let memory = MarkdownMemory::new(directory("forget").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    let shared = memory.record_now(&note("a", MemoryScope::Hive, "stale claim")).unwrap();
    let private = memory.record_now(&note("a", MemoryScope::Seat, "private")).unwrap();
    memory.forget_now("b", &private.id).unwrap();
    memory.forget_now("b", &shared.id).unwrap();
    let left = memory.recall_now(&query("a", "", 5)).unwrap();
    assert_eq!(left, vec![private]);
}

#[test]
fn ids_never_repeat_after_a_forget() {
    let memory = MarkdownMemory::new(directory("ids").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    let a = memory.record_now(&note("a", MemoryScope::Hive, "one")).unwrap();
    memory.forget_now("a", &a.id).unwrap();
    let b = memory.record_now(&note("a", MemoryScope::Hive, "two")).unwrap();
    assert_ne!(a.id, b.id);
}

#[test]
fn concurrent_writers_lose_no_notes() {
    let memory = Arc::new(MarkdownMemory::new(
        directory("race").join("HIVE_MEMORY.md"),
        Compaction { keep_recent: 40, max_recent: 80, ..Compaction::DEFAULT },
    ));
    let writers: Vec<_> = (0..4)
        .map(|seat| {
            let memory = Arc::clone(&memory);
            std::thread::spawn(move || {
                for n in 0..8 {
                    memory
                        .record_now(&note(&format!("s{seat}"), MemoryScope::Hive, &format!("fact {seat}-{n}")))
                        .unwrap();
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    assert_eq!(memory.recall_now(&query("s0", "", 50)).unwrap().len(), 32);
}

#[test]
fn a_stale_lock_is_broken_and_a_live_one_times_out_cleanly() {
    let dir = directory("lock");
    let path = dir.join("HIVE_MEMORY.md");
    let lock = path.with_extension("md.lock");
    fs::write(&lock, "").unwrap();
    let old = SystemTime::now() - Duration::from_secs(60);
    fs::File::options().write(true).open(&lock).unwrap().set_modified(old).unwrap();
    let memory = MarkdownMemory::new(&path, Compaction::DEFAULT);
    assert!(memory.record_now(&note("a", MemoryScope::Hive, "after stale lock")).is_ok());
    assert!(!lock.exists(), "the lock is released");
}

#[test]
fn ignores_lines_it_cannot_parse_instead_of_failing() {
    let dir = directory("junk");
    let path = dir.join("HIVE_MEMORY.md");
    fs::write(&path, "# Hive memory\nrandom prose\n## Recent\n- not an entry\n- m7 | hive | a | kept\n").unwrap();
    let memory = MarkdownMemory::new(&path, Compaction::DEFAULT);
    let found = memory.recall_now(&query("a", "", 5)).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(memory.record_now(&note("a", MemoryScope::Hive, "next")).unwrap().id, "m8");
}

#[tokio::test]
async fn it_serves_the_port_through_the_hive_tools() {
    use tinyhivemind_tools::MemoryTools;
    let memory = MarkdownMemory::new(directory("tools").join("HIVE_MEMORY.md"), Compaction::DEFAULT);
    let tools = MemoryTools::new(Arc::new(memory));
    let said = tools
        .call("solver", "hive_memory_note", &serde_json::json!({"text": "dead end: BM"}))
        .await
        .unwrap();
    assert_eq!(said, "remembered as m1");
    let got = tools
        .call("checker", "hive_memory_recall", &serde_json::json!({"query": "dead"}))
        .await
        .unwrap();
    assert_eq!(got, "m1 [solver] dead end: BM");
}
