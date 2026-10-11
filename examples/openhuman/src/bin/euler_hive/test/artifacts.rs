//! Run-directory artifacts: safe names, numbering and failed appends.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use tinyhivemind_core::embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingRequest, RoutingSource,
};

use super::super::agents::system_prompt;
use super::super::floors::{Assignment, Federation, Row};
use super::super::memory::{run_scoped, scope_binding};
use super::super::package::{self, SeatTool};
use super::super::router::addressed;
use super::super::sandbox::docker_args;
use super::super::search::{Provider, condense_exa};
use super::super::tools::{SeatScope, schema};
use super::super::turn::{TurnInput, prompt};
use super::support::*;

#[test]
fn a_problem_id_is_one_safe_path_component() {
    assert_eq!(super::super::path_component("pe1014").expect("plain id"), "pe1014");
    for bad in ["", "..", "../x", "/tmp/x", "a/b", "a b"] {
        assert!(super::super::path_component(bad).is_err(), "{bad:?} accepted");
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
    let error = super::super::artifacts::append_to(&mut Broken, &serde_json::json!({"type": "end"}))
        .expect_err("a broken stream fails");
    assert!(error.to_string().contains("disk full"));
    let dir = tempfile::tempdir().expect("dir");
    let run = super::super::artifacts::RunDir::create(dir.path().join("run")).expect("run dir");
    run.note("frames.ndjson", Err(error));
    assert!(run.check().is_err());
}
