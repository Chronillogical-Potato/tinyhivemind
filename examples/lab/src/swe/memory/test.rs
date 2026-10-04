//! Hive memory over the in-memory reference engine, a server that never
//! answers, and (when `CORTEX_DB_URL` is set) a live CortexDB.

use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use tinymemory_api::conformance::ReferenceEngine;
use tinymemory_api::{ForgetTarget, MemoryEngine};

use super::*;

const SEATS: [&str; 3] = ["lead", "implementer", "tester"];

fn failed_pytest() -> Remembered {
    Remembered {
        text: "pytest still fails; the import path is wrong".into(),
        ledger: vec![LedgerEntry {
            cmd: "pytest tests/test_parser.py -x".into(),
            exit: Some(1),
            outcome: "ModuleNotFoundError: No module named 'parser_core'".into(),
        }],
    }
}

fn memory_on(engine: Arc<dyn MemoryEngine>, run: &str) -> HiveMemory {
    HiveMemory::new(engine, run, &SEATS, 1200).expect("memory")
}

fn start(focus: &str) -> Moment {
    Moment::SessionStart {
        focus: focus.into(),
    }
}

#[test]
fn the_namespace_root_is_pinned_to_the_run_id() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "trial 7/a");
    assert_eq!(memory.root().to_string(), "team:trial-7-a");
    assert_eq!(
        memory
            .layout
            .conversations("implementer")
            .expect("node")
            .to_string(),
        "team:trial-7-a/agent:implementer"
    );
    assert!(run_root("///").is_err());
    assert!(generated_run_id("hive").starts_with("hive-"));
}

#[test]
fn a_seats_later_recall_surfaces_its_earlier_failed_attempt() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-fail");
    let stored = memory.remember("implementer", &failed_pytest());
    assert_eq!(stored.error, None);
    assert_eq!(stored.items, 1);
    let recalled = memory.recall("implementer", &start("run pytest on the parser"));
    assert_eq!(recalled.report.error, None);
    let pack = recalled.pack.expect("a pack");
    assert!(pack.starts_with(MEMORY_HEADER));
    assert!(pack.contains("FAILED attempt, exit 1"), "{pack}");
    assert!(pack.contains("ModuleNotFoundError"));
}

#[test]
fn a_rejoin_shows_a_teammates_new_memory_once() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-rejoin");
    memory.remember("implementer", &failed_pytest());
    let rejoin = Moment::Rejoin {
        focus: "pytest parser".into(),
    };
    let first = memory.recall("tester", &rejoin);
    let pack = first.pack.expect("the teammate's attempt");
    assert!(pack.contains("@implementer") && pack.contains("pytest"));
    let again = memory.recall("tester", &rejoin);
    assert!(
        again
            .pack
            .is_none_or(|p| !p.contains("ModuleNotFoundError")),
        "already shown"
    );
    let own = memory.recall("implementer", &rejoin);
    assert!(
        own.pack.is_none_or(|p| !p.contains("ModuleNotFoundError")),
        "a rejoin never repeats the seat's own history"
    );
}

#[test]
fn a_compaction_recall_carries_the_seats_thread() {
    let memory = memory_on(Arc::new(ReferenceEngine::new()), "t-compact");
    memory.remember("implementer", &failed_pytest());
    let recalled = memory.recall(
        "implementer",
        &Moment::Compaction {
            dropped: vec!["assistant: ran pytest".into()],
            focus: "fix the import".into(),
        },
    );
    assert_eq!(recalled.report.moment, "compaction");
    assert!(recalled.pack.expect("pack").contains("pytest"));
}

#[test]
fn two_run_ids_share_nothing() {
    let engine: Arc<dyn MemoryEngine> = Arc::new(ReferenceEngine::new());
    let one = memory_on(engine.clone(), "run-one");
    let two = memory_on(engine, "run-two");
    one.remember("implementer", &failed_pytest());
    let other = two.recall("implementer", &start("pytest"));
    assert_eq!(other.pack, None);
    assert_eq!(other.report.error, None);
}

#[test]
fn packs_are_clipped_to_the_budget() {
    let long = format!("# Memory\n\n## Learnings\n{}", "- fact\n".repeat(400));
    let framed = frame(&long, 50).expect("pack");
    assert!(framed.chars().count() <= 200);
    assert!(framed.starts_with(MEMORY_HEADER) && !framed.contains("# Memory\n"));
    assert_eq!(frame("# Memory", 50), None);
}

/// A CortexDB address whose server accepts connections and never answers.
fn silent_server() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    (listener, url)
}

fn quick() -> Timeouts {
    Timeouts {
        recall: Duration::from_millis(300),
        remember: Duration::from_millis(300),
        finish: Duration::from_millis(300),
    }
}

#[test]
fn a_timeout_degrades_to_no_memory_with_the_reason() {
    let (_held, url) = silent_server();
    let memory = HiveMemory::cortex(&url, "k", "t-slow", &SEATS, 1200)
        .expect("memory")
        .with_timeouts(quick());
    let recalled = memory.recall("lead", &start("anything"));
    assert_eq!(recalled.pack, None);
    assert!(recalled.report.error.expect("error").contains("timed out"));
    let stored = memory.remember("lead", &failed_pytest());
    assert!(stored.error.expect("error").contains("timed out"));
    assert!(memory.finish().is_empty());
}

#[test]
fn an_unreachable_server_degrades_to_no_memory() {
    let (listener, url) = silent_server();
    drop(listener);
    let memory = HiveMemory::cortex(&url, "k", "t-down", &SEATS, 1200)
        .expect("memory")
        .with_timeouts(quick());
    let stored = memory.remember("lead", &failed_pytest());
    assert!(stored.error.is_some(), "{stored:?}");
    assert_eq!(memory.recall("lead", &start("x")).pack, None);
    assert!(HiveMemory::cortex("not a url", "k", "t", &SEATS, 1).is_err());
}

#[test]
fn a_bad_seat_or_run_id_is_refused_up_front() {
    let engine: Arc<dyn MemoryEngine> = Arc::new(ReferenceEngine::new());
    assert!(HiveMemory::new(engine.clone(), "ok", &["  "], 10).is_err());
    assert!(HiveMemory::new(engine, "%%%", &SEATS, 10).is_err());
}

/// Store and recall against a real CortexDB when `CORTEX_DB_URL` is set
/// (key from `CORTEX_DB_KEY`); a no-op otherwise. Cleans up after itself.
#[test]
fn live_cortex_memory_round_trip() {
    let Ok(url) = std::env::var("CORTEX_DB_URL") else {
        return;
    };
    let key = std::env::var("CORTEX_DB_KEY").unwrap_or_default();
    let run = generated_run_id("live");
    let memory = HiveMemory::cortex(&url, &key, &run, &SEATS, 1200).expect("memory");
    eprintln!("live: {}", memory.describe());
    let stored = memory.remember("implementer", &failed_pytest());
    eprintln!("live: {}", stored.detail("implementer"));
    assert_eq!(stored.error, None);
    let mut found = None;
    for _ in 0..10 {
        let recalled = memory.recall(
            "tester",
            &Moment::Rejoin {
                focus: "pytest parser import".into(),
            },
        );
        eprintln!("live: {}", recalled.report.detail("tester"));
        if let Some(pack) = recalled.pack.filter(|p| p.contains("pytest")) {
            found = Some(pack);
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let compaction = memory.recall(
        "implementer",
        &Moment::Compaction {
            dropped: vec!["ran pytest".into()],
            focus: "the import path".into(),
        },
    );
    eprintln!("live: {}", compaction.report.detail("implementer"));
    let filter = memory.layout.holistic_filter();
    let engine = memory.engine.clone();
    let forgotten = memory
        .runtime
        .block_on(async { engine.forget(ForgetTarget::Filter(filter)).await });
    eprintln!("live: cleanup {forgotten:?}");
    let pack = found.expect("the teammate's attempt is recalled");
    eprintln!("live pack:\n{pack}");
    assert!(pack.contains("pytest"));
}
