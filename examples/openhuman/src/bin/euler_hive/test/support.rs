//! Fixtures shared by the test modules.

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


pub(super) fn package_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/tinyhivemind-lang/examples/hives/euler-hive")
}

pub(super) fn lowered() -> (tinyhivemind_lang::Package, tinyhivemind_lang::LoweredHive) {
    let documents = package::read_package(&package_dir()).expect("package reads");
    package::load(documents, "Find the answer to a test problem.").expect("package lowers")
}

pub(super) fn scope(desk: &str) -> SeatScope {
    SeatScope {
        seat: "s".into(),
        desk: desk.into(),
        colleagues: vec!["a".into(), "b".into()],
        linked_desks: vec!["verify".into()],
        conversation: "c".into(),
    }
}

/// Drive a federation offline: `act` chooses each seat's action from its
/// desk and the reason it was called. Returns the rounds run.
pub(super) async fn drive(
    federation: &mut Federation,
    act: impl Fn(&str, &str, &str) -> super::tools::Action,
) -> usize {
    let mut rounds = 0;
    while federation.finished.is_none() && federation.undecided.is_none() && rounds < 60 {
        let pending = federation.ready().expect("ready");
        if pending.is_empty() {
            break;
        }
        for round in pending {
            let desk = federation.floors[round.desk].desk.id.clone();
            let at = federation.floors[round.desk].journal.len();
            let results = round
                .seats
                .iter()
                .map(|seat| {
                    let reason = federation.assignment(round.desk, seat).reason;
                    (seat.clone(), act(&desk, seat, &reason))
                })
                .collect();
            federation
                .commit(round.desk, at, results)
                .await
                .expect("commit");
        }
        rounds += 1;
    }
    rounds
}

pub(super) fn complete(report: &str) -> super::tools::Action {
    super::tools::Action::Complete {
        report: report.into(),
        candidate: None,
        verdict: None,
    }
}

pub(super) fn refer(desk: &str) -> super::tools::Action {
    super::tools::Action::Refer {
        desk: desk.into(),
        message: "please check".into(),
    }
}

pub(super) fn propose(candidate: &str, verdict: Option<super::tools::Verdict>) -> super::tools::Action {
    super::tools::Action::Complete {
        report: "worked".into(),
        candidate: Some(candidate.into()),
        verdict,
    }
}
