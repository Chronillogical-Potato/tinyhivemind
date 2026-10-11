//! A `hive.json` may omit every section with a safe default, and the
//! canonical files the repository already ships still decode into exactly
//! what they spell out.
use std::{collections::BTreeMap, fs, path::Path};
use tinyhivemind_lang::{
    Constitution, CoordinatorSettings, Manifest, MemorySpec, Policies, Rule, diagnose_documents,
    lower, parse, validate,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The smallest package worth writing: one role, one seat, one desk, and
/// nothing else spelled out.
fn minimal() -> BTreeMap<String, String> {
    let hive = r#"{
  "$schema": "https://tinyhumans.ai/schemas/hive/v1",
  "version": 1,
  "id": "mini",
  "name": "Mini",
  "roles": [{ "id": "solver", "charter": "roles/solver.md" }],
  "seats": [
    { "id": "solver", "role": "solver", "prompt": "seats/solver.md", "template": "default" }
  ],
  "desks": [{ "id": "room", "name": "Room", "members": ["solver"] }]
}"#;
    BTreeMap::from([
        ("hive.json".to_owned(), hive.to_owned()),
        ("constitution.json".to_owned(), "{}".to_owned()),
        ("memory.json".to_owned(), r#"{ "root": "team:mini" }"#.to_owned()),
        ("roles/solver.md".to_owned(), "Solve the problem.\n".to_owned()),
        ("seats/solver.md".to_owned(), "Show your working.\n".to_owned()),
    ])
}

fn with_hive(edit: impl FnOnce(&mut serde_json::Value)) -> Result<BTreeMap<String, String>> {
    let mut documents = minimal();
    let mut hive: serde_json::Value = serde_json::from_str(&documents["hive.json"])?;
    edit(&mut hive);
    documents.insert("hive.json".into(), hive.to_string());
    Ok(documents)
}

fn findings(documents: &BTreeMap<String, String>) -> Vec<(String, Option<String>, Rule)> {
    diagnose_documents(documents)
        .1
        .into_iter()
        .map(|d| (d.file, d.pointer, d.rule))
        .collect()
}

#[test]
fn a_minimal_hive_parses_validates_and_lowers_on_defaults() -> Result<()> {
    let package = parse(&minimal())?;
    validate(&package)?;
    let hive = lower(&package)?;

    let manifest = &package.manifest;
    assert_eq!(manifest.policies, Policies::default());
    assert_eq!(manifest.coordinator, CoordinatorSettings::default());
    assert!(manifest.goals.is_empty() && manifest.context.is_empty());
    let seat = &manifest.seats[0];
    assert_eq!(seat.label, "solver");
    assert_eq!(seat.config, serde_json::json!({}));
    assert_eq!(seat.model, None);
    assert!(seat.tools.allow.is_empty() && seat.mcp.is_empty() && seat.skills.is_empty());
    assert_eq!(manifest.desks[0].description, None);
    assert_eq!(package.constitution, Constitution::default());
    assert_eq!(
        package.memory,
        MemorySpec {
            root: "team:mini".into(),
            ..MemorySpec::default()
        }
    );
    assert_eq!(hive.seats[0].id, "solver");
    assert_eq!(hive.desks[0].members, ["solver"]);
    Ok(())
}

#[test]
fn a_minimal_hive_serializes_as_its_complete_canonical_form() -> Result<()> {
    let package = parse(&minimal())?;
    let written = serde_json::to_value(&package.manifest)?;
    for key in [
        "goals",
        "missions",
        "topology",
        "referrals",
        "context",
        "policies",
        "coordinator",
    ] {
        assert!(written.get(key).is_some(), "{key} is written");
    }
    assert_eq!(written["seats"][0]["label"], "solver");
    assert_eq!(written["policies"]["conduct"]["turn_wall"], 60);

    let mut spelled = minimal();
    spelled.insert("hive.json".into(), written.to_string());
    assert_eq!(parse(&spelled)?, package);
    Ok(())
}

#[test]
fn an_omitted_policy_field_keeps_its_default_beside_one_that_is_set() -> Result<()> {
    let documents = with_hive(|hive| {
        hive["policies"] = serde_json::json!({
            "episode": { "turn_budget": 20, "quorum": { "threshold": 3 } },
            "digest": { "budget_chars": 8000 }
        });
        hive["coordinator"] = serde_json::json!({ "round_width": 4 });
    })?;
    let manifest = parse(&documents)?.manifest;
    let defaults = Policies::default();
    let episode = manifest.policies.episode;
    assert_eq!(episode.turn_budget, 20);
    assert_eq!(episode.quorum.threshold, 3);
    assert_eq!(episode.quorum.window, defaults.episode.quorum.window);
    assert_eq!(episode.round_width, defaults.episode.round_width);
    assert_eq!(manifest.policies.digest.budget_chars, 8000);
    assert_eq!(manifest.policies.routing, defaults.routing);
    assert_eq!(manifest.coordinator.round_width, 4);
    assert_eq!(
        manifest.coordinator.conduct_policy,
        CoordinatorSettings::default().conduct_policy
    );
    Ok(())
}

#[test]
fn a_defaulted_policy_still_answers_to_the_constitution() -> Result<()> {
    let mut documents = with_hive(|hive| {
        hive["policies"] = serde_json::json!({ "episode": { "round_width": 4 } });
    })?;
    // `round_width` may not exceed the defaulted `revealed_width` of 1.
    assert_eq!(
        findings(&documents),
        [(
            "hive.json".into(),
            Some("/policies/episode/round_width".into()),
            Rule::Bound
        )]
    );
    // A constitution tighter than the defaults assume is enforced too: the
    // default conduct turn walls of 60 exceed a cap of 30 turns.
    documents = minimal();
    documents.insert("constitution.json".into(), r#"{"max_turns": 30}"#.into());
    assert_eq!(
        findings(&documents),
        [
            (
                "hive.json".into(),
                Some("/policies/conduct".into()),
                Rule::Bound
            ),
            (
                "hive.json".into(),
                Some("/coordinator/conduct_policy".into()),
                Rule::Bound
            ),
        ]
    );
    Ok(())
}

#[test]
fn unknown_fields_are_still_rejected_where_fields_are_optional() -> Result<()> {
    let cases: [(fn(&mut serde_json::Value), Option<&str>); 5] = [
        (|h| h["seats"][0]["lable"] = "x".into(), None),
        (|h| h["desks"][0]["x"] = 1.into(), Some("/desks/0/x")),
        (
            |h| h["policies"] = serde_json::json!({ "episode": { "x": 1 } }),
            Some("/policies/episode/x"),
        ),
        (
            |h| h["policies"] = serde_json::json!({ "division": { "x": 1 } }),
            None,
        ),
        (|h| h["coordinator"] = serde_json::json!({ "x": 1 }), None),
    ];
    for (edit, pointer) in cases {
        let documents = with_hive(edit)?;
        assert_eq!(
            findings(&documents),
            [("hive.json".into(), pointer.map(str::to_owned), Rule::Json)]
        );
    }
    let mut documents = minimal();
    documents.insert("constitution.json".into(), r#"{"max_turn": 3}"#.into());
    assert_eq!(
        findings(&documents),
        [("constitution.json".into(), None, Rule::Json)]
    );
    Ok(())
}

#[test]
fn an_array_is_not_accepted_for_a_defaulted_object() -> Result<()> {
    let documents = with_hive(|hive| hive["seats"][0]["tools"] = serde_json::json!([]))?;
    assert_eq!(
        findings(&documents),
        [(
            "hive.json".into(),
            Some("/seats/0/tools".into()),
            Rule::Json
        )]
    );
    Ok(())
}

#[test]
fn required_fields_stay_required() -> Result<()> {
    for (file, source) in [
        ("hive.json", r#"{"$schema": "https://tinyhumans.ai/schemas/hive/v1", "version": 1, "id": "x", "name": "x"}"#),
        ("memory.json", "{}"),
    ] {
        let mut documents = minimal();
        documents.insert(file.into(), source.into());
        let found = diagnose_documents(&documents).1;
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].file.as_str(), found[0].rule), (file, Rule::Json));
        assert!(found[0].message.contains("missing field"), "{}", found[0].message);
    }
    let documents = with_hive(|hive| {
        hive["seats"][0].as_object_mut().map(|seat| seat.remove("template"));
    })?;
    assert_eq!(
        findings(&documents),
        [("hive.json".into(), None, Rule::Json)]
    );
    Ok(())
}

#[test]
fn shipped_canonical_files_reserialize_to_exactly_their_contents() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for parent in ["tests/fixtures", "examples/hives"] {
        for entry in fs::read_dir(root.join(parent))? {
            let dir = entry?.path();
            if !dir.join("hive.json").is_file() {
                continue;
            }
            same::<Manifest>(&dir.join("hive.json"))?;
            same::<Constitution>(&dir.join("constitution.json"))?;
            same::<MemorySpec>(&dir.join("memory.json"))?;
            checked += 1;
        }
    }
    assert!(checked >= 10, "found {checked} packages");
    Ok(())
}

/// The decoded file re-serializes to the same JSON value: no field dropped,
/// none added by a default. Bytes can differ only in the key order of opaque
/// objects such as a seat's `config`, which `serde_json` writes sorted.
fn same<T: serde::de::DeserializeOwned + serde::Serialize>(path: &Path) -> Result<()> {
    let source = fs::read_to_string(path)?;
    let decoded: T = serde_json::from_str(&source)?;
    let input: serde_json::Value = serde_json::from_str(&source)?;
    assert_eq!(
        serde_json::to_value(&decoded)?,
        input,
        "{} round-trips",
        path.display()
    );
    Ok(())
}
