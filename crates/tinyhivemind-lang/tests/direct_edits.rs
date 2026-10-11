//! Direct edits to the gallery's `self-improving` package, checked the way an
//! orchestrator's edits would be: whole files in, diagnostics or a review out.
//!
//! The package is read with `include_str!`, so this suite follows the
//! gallery rather than a copy of it.
use std::collections::BTreeMap;
use tinyhivemind_lang::{
    Diagnostic, Review, Rule, check, check_packages, diagnose, diagnose_documents,
    lineage::{self, Direction, ObjectiveScore, Verdict, VersionMetadata},
    parse,
    patch::{self, PatchClass},
    validate,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

macro_rules! package {
    ($($path:literal),* $(,)?) => {
        BTreeMap::from([$((
            $path.to_owned(),
            include_str!(concat!("../examples/hives/self-improving/", $path)).to_owned(),
        )),*])
    };
}

fn original() -> BTreeMap<String, String> {
    package![
        "hive.json",
        "constitution.json",
        "memory.json",
        "README.md",
        "context/playbook.md",
        "roles/README.md",
        "roles/critic.md",
        "roles/editor.md",
        "roles/solver.md",
        "seats/README.md",
        "seats/critic.md",
        "seats/editor.md",
        "seats/solver.md",
    ]
}

/// The original with `path` rewritten by `edit`.
fn edited(path: &str, edit: impl FnOnce(&str) -> String) -> BTreeMap<String, String> {
    let mut files = original();
    let next = edit(&files[path]);
    files.insert(path.to_owned(), next);
    files
}

/// Rewrite one JSON file through its decoded value.
fn edit_json(
    files: &mut BTreeMap<String, String>,
    path: &str,
    edit: impl FnOnce(&mut serde_json::Value),
) -> Result<()> {
    let mut value: serde_json::Value = serde_json::from_str(&files[path])?;
    edit(&mut value);
    files.insert(path.to_owned(), serde_json::to_string_pretty(&value)?);
    Ok(())
}

type Checked = std::result::Result<Review, Vec<Diagnostic>>;

fn refusal(result: Checked) -> Result<Vec<Diagnostic>> {
    result.err().ok_or_else(|| "expected the edit to be refused".into())
}

/// The review of an accepted edit, or its diagnostics as the test failure.
fn accepted(result: Checked) -> Result<Review> {
    result.map_err(|diagnostics| {
        let lines: Vec<_> = diagnostics.iter().map(ToString::to_string).collect();
        lines.join("\n").into()
    })
}

fn located(diagnostics: &[Diagnostic]) -> Vec<(&str, Option<&str>, Rule)> {
    diagnostics
        .iter()
        .map(|d| (d.file.as_str(), d.pointer.as_deref(), d.rule))
        .collect()
}

#[test]
fn a_prompt_edit_is_accepted_as_one_prompt_change() -> Result<()> {
    let files = edited("seats/solver.md", |body| {
        format!("{body}Include canceled invoices.\n")
    });
    let review = accepted(check(&original(), &files))?;
    assert_eq!(review.changes.len(), 1);
    assert_eq!(review.changes[0].class, PatchClass::Prompt);
    // The recorded form is exactly what `diff` derives for the class.
    let base = parse(&original())?;
    assert_eq!(
        review.changes[0],
        patch::diff(&base, &review.candidate, PatchClass::Prompt)?
    );
    assert_eq!(review.candidate, parse(&files)?);
    Ok(())
}

#[test]
fn a_policy_edit_is_refused_at_the_field_it_changed() -> Result<()> {
    let mut files = original();
    edit_json(&mut files, "hive.json", |hive| {
        hive["policies"]["episode"]["turn_budget"] = 12.into();
    })?;
    let diagnostics = refusal(check(&original(), &files))?;
    assert_eq!(
        located(&diagnostics),
        [(
            "hive.json",
            Some("/policies/episode/turn_budget"),
            Rule::ForbiddenClass
        )]
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "hive.json:/policies/episode/turn_budget: forbidden_class: \
         the constitution does not permit policy edits"
    );
    Ok(())
}

#[test]
fn every_violation_of_one_edit_is_reported_together() -> Result<()> {
    let mut files = edited("context/playbook.md", |playbook| {
        playbook.replace("round only", "HELDOUT-7F3A round only")
    });
    edit_json(&mut files, "constitution.json", |constitution| {
        constitution["max_turns"] = 120.into();
    })?;
    edit_json(&mut files, "hive.json", |hive| {
        hive["missions"][0]["role"] = "ghost".into();
    })?;
    let diagnostics = refusal(check(&original(), &files))?;
    assert_eq!(
        located(&diagnostics),
        [
            ("constitution.json", Some("/max_turns"), Rule::Immutable),
            ("context/playbook.md", None, Rule::Contamination),
            ("hive.json", Some("/missions/0/role"), Rule::Reference),
        ]
    );
    Ok(())
}

#[test]
fn a_multi_class_edit_reproduces_through_its_derived_patches() -> Result<()> {
    let mut files = edited("seats/critic.md", |body| {
        format!("{body}Name the invoice id.\n")
    });
    files.insert(
        "context/playbook.md".into(),
        files["context/playbook.md"].replace("integer cents", "whole cents"),
    );
    edit_json(&mut files, "hive.json", |hive| {
        hive["goals"][0]["acceptance"]
            .as_array_mut()
            .map(|criteria| criteria.push("Shows every line item".into()));
    })?;
    let review = accepted(check(&original(), &files))?;
    let classes: Vec<_> = review.changes.iter().map(|change| change.class).collect();
    assert_eq!(
        classes,
        [PatchClass::Prompt, PatchClass::Context, PatchClass::Goal]
    );
    // One lineage record per change, chained, ends at the edited package.
    let mut current = parse(&original())?;
    let mut parent = None;
    let mut ids = Vec::new();
    for change in &review.changes {
        current = patch::apply(&current, change)?.candidate;
        let watermarks = BTreeMap::from([("solver-notes".into(), 0), ("eval-ledger".into(), 0)]);
        let record = lineage::record(
            &current,
            VersionMetadata {
                parent: parent.clone(),
                patch: change.clone(),
                rationale: "direct edit".into(),
                transcript_refs: vec![],
                scores: BTreeMap::from([(
                    "heldout".into(),
                    ObjectiveScore {
                        value: 3.0,
                        direction: Direction::Maximize,
                    },
                )]),
                verdict: Verdict::Accepted,
                memory_watermarks: watermarks.clone(),
            },
        )?;
        parent = Some(record.id.clone());
        ids.push(record.id);
    }
    assert_eq!(current, review.candidate);
    assert_eq!(current, parse(&files)?);
    let watermarks = BTreeMap::from([("solver-notes".into(), 0), ("eval-ledger".into(), 0)]);
    assert_eq!(ids.last(), Some(&lineage::content_id(&current, &watermarks)?));
    assert_eq!(ids.len(), 3);
    Ok(())
}

#[test]
fn unparseable_json_names_its_file_and_line() -> Result<()> {
    let files = edited("memory.json", |memory| memory.replacen("\"root\"", "root", 1));
    let diagnostics = refusal(check(&original(), &files))?;
    assert_eq!(located(&diagnostics), [("memory.json", None, Rule::Json)]);
    assert!(diagnostics[0].message.contains("line 2 column 3"), "{}", diagnostics[0]);
    Ok(())
}

#[test]
fn an_unowned_change_is_reported_rather_than_accepted() -> Result<()> {
    let mut files = edited("README.md", |readme| format!("{readme}\nEdited.\n"));
    edit_json(&mut files, "hive.json", |hive| hive["name"] = "Renamed".into())?;
    files.insert("notes.txt".into(), "scratch\n".into());
    let diagnostics = refusal(check(&original(), &files))?;
    assert_eq!(
        located(&diagnostics),
        [
            ("README.md", None, Rule::Unowned),
            ("hive.json", Some("/name"), Rule::Unowned),
            ("notes.txt", None, Rule::Unowned),
        ]
    );
    Ok(())
}

#[test]
fn diagnose_collects_several_validation_errors_at_once() -> Result<()> {
    let mut package = parse(&original())?;
    package.manifest.seats[0].role = "ghost".into();
    package.manifest.desks[0].members.push("phantom".into());
    package.manifest.policies.episode.turn_budget = 0;
    let diagnostics = diagnose(&package);
    assert_eq!(
        located(&diagnostics),
        [
            ("hive.json", Some("/seats/0/role"), Rule::Reference),
            ("hive.json", Some("/desks/0/members/3"), Rule::Reference),
            ("hive.json", Some("/policies/episode/turn_budget"), Rule::Bound),
        ]
    );
    // `validate` still stops at, and returns, the first of them.
    let first = validate(&package).err().ok_or("expected a validation error")?;
    assert_eq!(first.to_string(), diagnostics[0].message);
    Ok(())
}

#[test]
fn diagnose_documents_reports_every_canonical_file_that_fails() {
    let mut files = original();
    files.insert("hive.json".into(), "{".into());
    files.remove("memory.json");
    let (package, diagnostics) = diagnose_documents(&files);
    assert!(package.is_none());
    assert_eq!(
        located(&diagnostics),
        [
            ("hive.json", None, Rule::Json),
            ("memory.json", None, Rule::MissingDocument),
        ]
    );
}

#[test]
fn check_packages_matches_check_on_the_decoded_files() -> Result<()> {
    let base = parse(&original())?;
    let mut edit = base.clone();
    edit.manifest.policies.episode.turn_budget = 12;
    let diagnostics = refusal(check_packages(&base, &edit))?;
    assert_eq!(
        located(&diagnostics),
        [(
            "hive.json",
            Some("/policies/episode/turn_budget"),
            Rule::ForbiddenClass
        )]
    );
    assert!(accepted(check_packages(&base, &base))?.changes.is_empty());
    Ok(())
}
