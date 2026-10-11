//! A small package touching every edit class, and helpers to edit its files.
use crate::{
    Diagnostic, Goal, Link, MemoryIdentity, MemoryKind, MemoryLifecycle, MemoryReach, Mission,
    Package, RecallMoment, Review, Role, Rule, Seat, SeatMemory, patch::PatchClass,
};
use std::collections::BTreeMap;
use tinyhivemind_core::desk::Desk;

pub(super) type Checked = Result<Review, Vec<Diagnostic>>;

/// A valid package: one seat, two desks, a goal, itemized context, an agent
/// memory and a pinned read-only ledger. Every class but `policy` is
/// permitted.
pub(super) fn package() -> Package {
    let mut p = Package::default();
    p.manifest.roles.push(Role {
        id: "writer".into(),
        charter: "roles/writer.md".into(),
    });
    p.manifest.seats.push(seat("alice"));
    for id in ["main", "side"] {
        p.manifest.desks.push(desk(id, &["alice"]));
    }
    p.manifest.topology.push(Link {
        from: "main".into(),
        to: "side".into(),
    });
    p.manifest.goals.push(Goal {
        id: "ship".into(),
        description: "Ship the change".into(),
        acceptance: vec!["tests pass".into()],
        subgoals: vec![],
    });
    p.manifest.missions.push(Mission {
        id: "deliver".into(),
        role: "writer".into(),
        goals: vec!["ship".into()],
    });
    p.manifest.context.push("context/notes.md".into());
    p.documents
        .insert("roles/writer.md".into(), "Write clearly.\n".into());
    p.documents.insert(
        "seats/alice.md".into(),
        "---\n{\"id\":\"alice\"}\n---\nDraft the change.\n".into(),
    );
    p.documents.insert(
        "context/notes.md".into(),
        "# Notes\n<!-- item:a -->\nFirst.\n<!-- item:b -->\nSecond.\n".into(),
    );
    p.memory.identities.push(identity("notes", "team:hive/agent:alice", false));
    let mut ledger = identity("ledger", "team:hive/project:ledger", true);
    ledger.kind = MemoryKind::Pool;
    p.memory.identities.push(ledger);
    p.memory.bindings.push(binding("alice", "notes"));
    p.constitution.permitted_edits = vec![
        PatchClass::Prompt,
        PatchClass::Context,
        PatchClass::Memory,
        PatchClass::Roster,
        PatchClass::Topology,
        PatchClass::Goal,
    ];
    p.constitution.read_only_memories = vec!["ledger".into()];
    p.constitution.contamination_canaries = vec!["CANARY-1".into()];
    p.constitution.evaluation = serde_json::json!({"kind": "exact"});
    p
}

pub(super) fn seat(id: &str) -> Seat {
    Seat {
        id: id.into(),
        label: id.into(),
        role: "writer".into(),
        prompt: format!("seats/{id}.md"),
        template: "agent".into(),
        config: serde_json::json!({}),
        ..Seat::default()
    }
}

pub(super) fn desk(id: &str, members: &[&str]) -> Desk {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": id,
        "description": null,
        "members": members,
        "responder_mode": "auto"
    }))
    .expect("a desk literal decodes")
}

pub(super) fn identity(id: &str, namespace: &str, read_only: bool) -> MemoryIdentity {
    MemoryIdentity {
        id: id.into(),
        namespace: namespace.into(),
        kind: MemoryKind::Agent,
        lifecycle: MemoryLifecycle::Hive,
        read_only,
        fork_parent: None,
    }
}

pub(super) fn binding(seat: &str, identity: &str) -> SeatMemory {
    SeatMemory {
        seat: seat.into(),
        identity: identity.into(),
        reads: vec![MemoryReach {
            at: format!("team:hive/agent:{seat}"),
            inherit: false,
            descendants: false,
        }],
        recall_at: vec![RecallMoment::SessionStart],
        budget_chars: 512,
        remember: vec![],
    }
}

/// A package as the document map an editor would see on disk.
pub(super) fn files(package: &Package) -> BTreeMap<String, String> {
    let mut files = package.documents.clone();
    for (path, value) in [
        ("hive.json", serde_json::to_string_pretty(&package.manifest)),
        (
            "constitution.json",
            serde_json::to_string_pretty(&package.constitution),
        ),
        ("memory.json", serde_json::to_string_pretty(&package.memory)),
    ] {
        files.insert(path.into(), value.expect("wire types serialize"));
    }
    files
}

/// Check `edit` of the fixture, by files.
pub(super) fn check_edit(edit: impl FnOnce(&mut Package)) -> Checked {
    let base = package();
    let mut edited = base.clone();
    edit(&mut edited);
    crate::check(&files(&base), &files(&edited))
}

/// The diagnostics of a refused check.
pub(super) fn refused(result: Checked) -> Vec<Diagnostic> {
    result.expect_err("expected the edit to be refused")
}

/// The review of an accepted check, failing with its diagnostics otherwise.
pub(super) fn accepted(result: Checked) -> Review {
    match result {
        Ok(review) => review,
        Err(diagnostics) => panic!("edit refused: {diagnostics:#?}"),
    }
}

pub(super) fn located(diagnostics: &[Diagnostic]) -> Vec<(&str, Option<&str>, Rule)> {
    diagnostics
        .iter()
        .map(|d| (d.file.as_str(), d.pointer.as_deref(), d.rule))
        .collect()
}

pub(super) fn classes(review: &Review) -> Vec<PatchClass> {
    review.changes.iter().map(|change| change.class).collect()
}

#[test]
fn the_fixture_is_valid() {
    assert_eq!(crate::diagnose(&package()), []);
    assert_eq!(crate::parse(&files(&package())), Ok(package()));
}
