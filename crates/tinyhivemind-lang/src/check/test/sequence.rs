//! Derived changes: one patch per class, in an order valid at every step.
use super::support::*;
use crate::{Rule, patch, patch::PatchClass};

#[test]
fn an_unchanged_package_needs_no_change() {
    let review = accepted(check_edit(|_| {}));
    assert_eq!((review.changes).len(), 0);
    assert_eq!(review.candidate, package());
}

#[test]
fn a_single_class_change_equals_what_diff_derives() {
    let review = accepted(check_edit(|p| {
        p.manifest.topology.clear();
    }));
    assert_eq!(classes(&review), [PatchClass::Topology]);
    let diffed = patch::diff(&package(), &review.candidate, PatchClass::Topology)
        .expect("diff derives the same change");
    assert_eq!(review.changes, [diffed]);
}

#[test]
fn a_dependent_change_is_ordered_after_what_it_depends_on() {
    // The binding names a seat the roster change adds, so memory, which comes
    // first in class order, can only apply after the roster.
    let review = accepted(check_edit(|p| {
        p.manifest.seats.push(seat("bob"));
        p.manifest.desks[0].members.push("bob".into());
        p.documents
            .insert("seats/bob.md".into(), "Review.\n".into());
        p.memory
            .identities
            .push(identity("bob-notes", "team:hive/agent:bob", false));
        p.memory.bindings.push(binding("bob", "bob-notes"));
    }));
    assert_eq!(classes(&review), [PatchClass::Roster, PatchClass::Memory]);
    let mut current = package();
    for change in &review.changes {
        current = patch::apply(&current, change)
            .expect("each derived change applies")
            .candidate;
    }
    assert_eq!(current, review.candidate);
}

#[test]
fn a_change_with_no_valid_order_is_a_sequence_finding() {
    // The new prompt's frontmatter restates a binding the memory change adds,
    // while that binding names the seat the roster change adds: neither step
    // is valid alone.
    let diagnostics = refused(check_edit(|p| {
        p.manifest.seats.push(seat("bob"));
        p.documents.insert(
            "seats/bob.md".into(),
            "---\n{\"memory\":\"bob-notes\"}\n---\nReview.\n".into(),
        );
        p.memory
            .identities
            .push(identity("bob-notes", "team:hive/agent:bob", false));
        p.memory.bindings.push(binding("bob", "bob-notes"));
    }));
    assert_eq!(
        located(&diagnostics),
        [("memory.json", None, Rule::Sequence)]
    );
    let message = &diagnostics[0].message;
    assert!(
        message.starts_with("no order of the memory, roster changes"),
        "{message}"
    );
}

#[test]
fn every_class_can_change_in_one_edit() {
    let mut base = package();
    base.constitution.permitted_edits.push(PatchClass::Policy);
    let mut edit = base.clone();
    edit.documents
        .insert("roles/writer.md".into(), "Write tersely.\n".into());
    edit.documents.insert(
        "context/notes.md".into(),
        "# Notes\n<!-- item:a -->\nFirst!\n<!-- item:b -->\nSecond.\n".into(),
    );
    edit.memory.bindings[0].budget_chars = 1024;
    edit.manifest.seats[0].label = "Alice".into();
    edit.manifest.referrals.push(crate::Link {
        from: "side".into(),
        to: "main".into(),
    });
    edit.manifest.policies.digest.budget_chars = 2000;
    edit.manifest.goals[0].description = "Ship it".into();
    let review = accepted(crate::check(&files(&base), &files(&edit)));
    assert_eq!(
        classes(&review),
        [
            PatchClass::Prompt,
            PatchClass::Context,
            PatchClass::Memory,
            PatchClass::Roster,
            PatchClass::Topology,
            PatchClass::Policy,
            PatchClass::Goal,
        ]
    );
    assert_eq!(review.candidate, edit);
}
