//! Run-directory artifacts: safe names, numbering and failed appends.

#[test]
fn a_problem_id_is_one_safe_path_component() {
    assert_eq!(
        super::super::path_component("pe1014").expect("plain id"),
        "pe1014"
    );
    for bad in ["", "..", "../x", "/tmp/x", "a/b", "a b"] {
        assert!(
            super::super::path_component(bad).is_err(),
            "{bad:?} accepted"
        );
    }
}

#[test]
fn artifact_names_cannot_leave_the_run_directory() {
    use super::super::artifacts::{contained, file_component};
    assert_eq!(file_component("verify-adversary"), "verify-adversary");
    assert_eq!(
        file_component("x/../../../../tmp/result"),
        "x_____________tmp_result"
    );
    assert_eq!(file_component(""), "_");
    assert!(contained("turns/0001-seat.md").is_ok());
    for bad in ["../x", "/tmp/x", "turns/../../x"] {
        assert!(contained(bad).is_err(), "{bad} accepted");
    }
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    run.write("turns/0001-seat.md", "ok");
    run.check().expect("a contained write lands");
    run.write("../escaped.md", "no");
    assert!(!dir.path().join("escaped.md").exists());
    assert!(
        run.check().is_err(),
        "a refused write is reported, not dropped"
    );
}

#[test]
fn every_turn_artifact_gets_its_own_number() {
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    let first = run.next_turn();
    let second = run.next_turn();
    assert_ne!(first, second);
}

#[test]
fn a_failed_frame_append_or_flush_is_reported() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("disk full"))
        }
    }
    let error =
        super::super::artifacts::append_to(&mut Broken, &serde_json::json!({"type": "end"}))
            .expect_err("a broken stream fails");
    assert!(error.to_string().contains("disk full"));
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    run.note("frames.ndjson", Err(error));
    assert!(run.check().is_err());
}

#[tokio::test]
async fn closing_the_run_fails_when_a_closing_artifact_could_not_be_written() {
    use std::collections::BTreeMap;

    use super::super::budget::{Budget, Limits};
    use super::super::floors::Federation;
    use super::super::report::{Closing, finish};

    let (_, hive) = super::support::lowered();
    let federation = Federation::new(&hive).expect("federation builds");
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    // The recording filesystem failed at some point before the closing writes.
    run.note("frames.ndjson", Err(std::io::Error::other("disk full")));
    let engine = tinymemory_api::conformance::reference::ReferenceEngine::new();
    let budget = Budget::new(Limits::SMOKE);
    let closed = finish(Closing {
        run: &run,
        federation: &federation,
        seats: &BTreeMap::new(),
        budget: &budget,
        engine: &engine,
        pool: None,
        stop: None,
        model: "test-model",
    })
    .await;
    let error = closed.expect_err("a truncated recording is not a clean exit");
    assert!(error.to_string().contains("disk full"), "{error}");
}

#[tokio::test]
async fn closing_an_exhausted_run_skips_optional_memory_waits() {
    use super::super::budget::{Budget, Limits};
    use super::super::floors::Federation;
    use super::super::report::{Closing, finish};
    use std::collections::BTreeMap;
    let (_, hive) = super::support::lowered();
    let federation = Federation::new(&hive).expect("federation");
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run");
    let engine = tinymemory_api::conformance::reference::ReferenceEngine::new();
    let budget = Budget::new(Limits {
        max_wall_seconds: 0,
        ..Limits::SMOKE
    });
    let seats = BTreeMap::new();
    let mut closing = Box::pin(finish(Closing {
        run: &run,
        federation: &federation,
        seats: &seats,
        budget: &budget,
        engine: &engine,
        pool: Some("not-a-namespace"),
        stop: Some("wall_clock".into()),
        model: "test",
    }));
    assert!(
        matches!(
            futures::poll!(closing.as_mut()),
            std::task::Poll::Ready(Ok(()))
        ),
        "finalization must not wait or read optional memory once the deadline is spent"
    );
}

#[test]
fn failed_initial_artifacts_prevent_opening_the_first_desk() {
    let (_, hive) = super::support::lowered();
    let mut federation = super::super::floors::Federation::new(&hive).expect("federation");
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run");
    std::fs::create_dir(run.path().join("package.json")).expect("block initial artifact");
    run.json("package.json", &serde_json::json!({"id":"test"}));
    assert!(super::super::open_federation(&mut federation, &run).is_err());
    assert!(
        federation.floors.iter().all(|floor| !floor.opened),
        "no seat work is authorized without the initial recording"
    );
}
