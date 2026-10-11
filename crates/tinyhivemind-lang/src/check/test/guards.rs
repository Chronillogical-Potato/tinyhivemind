//! The edit rules: immutability, class authority, canaries, read-only
//! memories and context deltas, each reported where it occurs.
use super::support::*;
use crate::{Rule, patch::PatchClass};
use serde_json::json;

#[test]
fn a_constitution_edit_is_immutable_at_each_changed_field() {
    let diagnostics = refused(check_edit(|p| {
        p.constitution.max_turns = 99;
        p.constitution.permitted_edits.push(PatchClass::Policy);
    }));
    assert_eq!(
        located(&diagnostics),
        [
            ("constitution.json", Some("/max_turns"), Rule::Immutable),
            (
                "constitution.json",
                Some("/permitted_edits/6"),
                Rule::Immutable
            ),
        ]
    );
    assert_eq!(
        diagnostics[0].message,
        "the constitution is host-owned and no edit may change it"
    );
}

#[test]
fn a_protected_manifest_key_is_immutable_even_under_a_permitted_class() {
    let diagnostics = refused(check_edit(|p| {
        p.manifest.seats[0].config = json!({"telemetry": {"enabled": false}});
    }));
    assert_eq!(
        located(&diagnostics),
        [(
            "hive.json",
            Some("/seats/0/config/telemetry"),
            Rule::Immutable
        )]
    );
    assert!(diagnostics[0].message.starts_with("telemetry settings"));
}

#[test]
fn a_protected_frontmatter_key_is_reported_on_its_markdown_file() {
    let diagnostics = refused(check_edit(|p| {
        p.documents.insert(
            "README.md".into(),
            "---\n{\"evaluation\":1}\n---\nNotes\n".into(),
        );
    }));
    assert_eq!(
        located(&diagnostics),
        [
            ("README.md", None, Rule::Immutable),
            ("README.md", None, Rule::Unowned),
        ]
    );
    assert!(
        diagnostics[0]
            .message
            .ends_with("(frontmatter /evaluation)")
    );
}

#[test]
fn changing_an_existing_frontmatter_is_immutable() {
    let diagnostics = refused(check_edit(|p| {
        p.documents.insert(
            "seats/alice.md".into(),
            "---\n{\"id\":\"alice\",\"label\":\"alice\"}\n---\nDraft the change.\n".into(),
        );
    }));
    assert_eq!(
        located(&diagnostics),
        [("seats/alice.md", None, Rule::Immutable)]
    );
    assert!(diagnostics[0].message.starts_with("frontmatter restates"));
}

#[test]
fn a_body_may_not_open_a_second_frontmatter() {
    let diagnostics = refused(check_edit(|p| {
        p.documents.insert(
            "seats/alice.md".into(),
            "---\n{\"id\":\"alice\"}\n---\n---\nsmuggled\n".into(),
        );
    }));
    assert_eq!(
        located(&diagnostics),
        [("seats/alice.md", None, Rule::Immutable)]
    );
    assert!(diagnostics[0].message.contains("second frontmatter"));
}

#[test]
fn an_unpermitted_class_is_refused_for_documents_and_fields() {
    let diagnostics = refused(check_edit(|p| {
        p.manifest.policies.digest.budget_chars = 10;
    }));
    assert_eq!(
        located(&diagnostics),
        [(
            "hive.json",
            Some("/policies/digest/budget_chars"),
            Rule::ForbiddenClass
        )]
    );
    let mut base = package();
    base.constitution.permitted_edits = vec![PatchClass::Prompt];
    let mut edit = base.clone();
    edit.manifest.seats.push(seat("bob"));
    edit.documents
        .insert("seats/bob.md".into(), "Review.\n".into());
    let diagnostics = refused(crate::check(&files(&base), &files(&edit)));
    assert_eq!(
        located(&diagnostics),
        [
            ("hive.json", Some("/seats/1"), Rule::ForbiddenClass),
            ("seats/bob.md", None, Rule::ForbiddenClass),
        ]
    );
    assert_eq!(
        diagnostics[1].message,
        "the constitution does not permit roster edits"
    );
}

#[test]
fn a_canary_is_located_in_json_values_keys_and_markdown() {
    let diagnostics = refused(check_edit(|p| {
        p.manifest.goals[0].description = "see CANARY-1".into();
        p.manifest.seats[0].config = json!({"CANARY-1": true});
        p.documents
            .insert("roles/writer.md".into(), "CANARY-1\n".into());
    }));
    assert_eq!(
        located(&diagnostics),
        [
            (
                "hive.json",
                Some("/goals/0/description"),
                Rule::Contamination
            ),
            ("roles/writer.md", None, Rule::Contamination),
        ]
    );
    let keyed = refused(check_edit(|p| {
        p.manifest.seats[0].config = json!({"CANARY-1": true});
    }));
    assert_eq!(
        located(&keyed),
        [(
            "hive.json",
            Some("/seats/0/config/CANARY-1"),
            Rule::Contamination
        )]
    );
}

#[test]
fn a_pinned_memory_and_its_bindings_are_read_only() {
    let diagnostics = refused(check_edit(|p| {
        p.memory.identities[1].lifecycle = crate::MemoryLifecycle::Run;
    }));
    assert_eq!(
        located(&diagnostics),
        [("memory.json", Some("/identities/1"), Rule::ReadOnly)]
    );
    assert_eq!(
        diagnostics[0].message,
        "memory ledger is pinned read-only by the constitution"
    );
    let bound = refused(check_edit(|p| {
        p.memory.bindings[0].identity = "ledger".into();
    }));
    assert_eq!(
        located(&bound),
        [("memory.json", Some("/bindings/0"), Rule::ReadOnly)]
    );
    let removed = refused(check_edit(|p| {
        p.memory.identities.pop();
    }));
    assert_eq!(
        located(&removed),
        [
            (
                "constitution.json",
                Some("/read_only_memories/0"),
                Rule::Reference
            ),
            ("memory.json", Some("/identities/1"), Rule::ReadOnly),
        ]
    );
}

#[test]
fn a_context_edit_must_be_expressible_as_item_deltas() {
    let notes = |body: &str| {
        check_edit(move |p| {
            p.documents.insert("context/notes.md".into(), body.into());
        })
    };
    let item = |result| {
        located(&refused(result))
            .first()
            .map(|d| (d.0.to_owned(), d.2))
    };
    let expected = Some(("context/notes.md".to_owned(), Rule::Context));
    // Text before the first item changed.
    assert_eq!(
        item(notes(
            "# Renamed\n<!-- item:a -->\nFirst.\n<!-- item:b -->\nSecond.\n"
        )),
        expected
    );
    // Items swapped.
    assert_eq!(
        item(notes(
            "# Notes\n<!-- item:b -->\nSecond.\n<!-- item:a -->\nFirst.\n"
        )),
        expected
    );
    // A new item before the last one.
    assert_eq!(
        item(notes(
            "# Notes\n<!-- item:a -->\nFirst.\n<!-- item:n -->\nNew.\n<!-- item:b -->\nSecond.\n"
        )),
        expected
    );
    // A last item without its newline.
    assert_eq!(
        item(notes(
            "# Notes\n<!-- item:a -->\nFirst.\n<!-- item:b -->\nSecond."
        )),
        expected
    );
    // A duplicate marker.
    assert_eq!(
        item(notes(
            "# Notes\n<!-- item:a -->\nFirst.\n<!-- item:a -->\nSecond.\n"
        )),
        expected
    );
    // An appended item is fine.
    let review = accepted(notes(
        "# Notes\n<!-- item:a -->\nFirst.\n<!-- item:b -->\nSecond.\n<!-- item:c -->\nThird.\n",
    ));
    assert_eq!(classes(&review), [PatchClass::Context]);
}

#[test]
fn a_new_context_document_must_carry_well_formed_markers() {
    let diagnostics = refused(check_edit(|p| {
        p.documents.insert(
            "context/extra.md".into(),
            "<!-- item:bad id -->\nx\n".into(),
        );
    }));
    assert_eq!(
        located(&diagnostics),
        [("context/extra.md", None, Rule::Context)]
    );
}

#[test]
fn identity_fields_no_class_owns_are_unowned() {
    let diagnostics = refused(check_edit(|p| {
        p.manifest.id = "renamed".into();
        p.manifest.version = 2;
    }));
    assert_eq!(
        located(&diagnostics),
        [
            ("hive.json", Some("/id"), Rule::Unowned),
            ("hive.json", Some("/version"), Rule::Schema),
            ("hive.json", Some("/version"), Rule::Unowned),
        ]
    );
    assert!(diagnostics[0].message.starts_with("no edit class owns id"));
}
