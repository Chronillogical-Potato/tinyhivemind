//! Checking by document map: an invalid original, files outside the
//! package, and edits that do not decode.
use super::support::*;
use crate::{Rule, check, check_packages};

#[test]
fn an_invalid_original_is_reported_as_the_original() {
    let mut base = package();
    base.manifest.seats[0].role = "ghost".into();
    let diagnostics = refused(check(&files(&base), &files(&package())));
    assert_eq!(
        located(&diagnostics),
        [("hive.json", Some("/seats/0/role"), Rule::Original)]
    );
    assert_eq!(
        diagnostics[0].message,
        "the original package is invalid: reference: unresolved role reference ghost"
    );
    let diagnostics = refused(check_packages(&base, &package()));
    assert_eq!(diagnostics[0].rule, Rule::Original);
    let mut broken = files(&package());
    broken.insert("hive.json".into(), "{".into());
    let diagnostics = refused(check(&broken, &files(&package())));
    assert_eq!(located(&diagnostics), [("hive.json", None, Rule::Original)]);
}

#[test]
fn a_file_outside_the_package_is_unowned_whether_added_or_removed() {
    let mut base = files(&package());
    base.insert("notes.txt".into(), "kept\n".into());
    base.insert("plan.txt".into(), "kept\n".into());
    let mut edit = base.clone();
    edit.remove("notes.txt");
    edit.insert("extra.yaml".into(), "a: 1\n".into());
    let diagnostics = refused(check(&base, &edit));
    assert_eq!(
        located(&diagnostics),
        [
            ("extra.yaml", None, Rule::Unowned),
            ("notes.txt", None, Rule::Unowned),
        ]
    );
}

#[test]
fn an_undecodable_edit_still_reports_constitution_and_canary_findings() {
    let base = package();
    let mut edit = files(&base);
    edit.insert("hive.json".into(), "{\"version\": }".into());
    let mut constitution = base.constitution.clone();
    constitution.max_width = 2;
    edit.insert(
        "constitution.json".into(),
        serde_json::to_string(&constitution).expect("constitution serializes"),
    );
    edit.insert("seats/alice.md".into(), "CANARY-1\n".into());
    edit.insert(
        "extra.json".into(),
        serde_json::json!({"notes": ["CANARY-1"]}).to_string(),
    );
    let diagnostics = refused(check(&files(&base), &edit));
    assert_eq!(
        located(&diagnostics),
        [
            ("constitution.json", Some("/max_width"), Rule::Immutable),
            ("extra.json", None, Rule::Unowned),
            ("extra.json", Some("/notes/0"), Rule::Contamination),
            ("hive.json", None, Rule::Json),
            ("seats/alice.md", None, Rule::Contamination),
        ]
    );
    assert!(diagnostics[3].message.contains("line 1 column 13"));
}

#[test]
fn a_constitution_that_is_not_an_object_is_immutable_as_a_whole() {
    let mut edit = files(&package());
    edit.insert("constitution.json".into(), "[]".into());
    let diagnostics = refused(check(&files(&package()), &edit));
    assert_eq!(
        located(&diagnostics),
        [
            ("constitution.json", None, Rule::Immutable),
            ("constitution.json", None, Rule::Json),
        ]
    );
}
