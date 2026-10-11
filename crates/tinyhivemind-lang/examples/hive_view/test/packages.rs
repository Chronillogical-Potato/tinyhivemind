//! Loading, summaries, rendering and the problem bank.

use crate::Result;
use crate::load::{documents, load, summary};
use crate::math::problems;
use crate::render::{mermaid, node};

use super::support::gallery;

#[test]
fn the_loader_rejects_a_directory_without_hive_json() {
    let error = documents(&gallery("review-board").join("roles"))
        .err()
        .map(|error| error.to_string());
    assert!(error.is_some_and(|error| error.contains("has no hive.json")));
}

#[test]
fn the_loader_rejects_a_path_that_is_not_a_directory() {
    let error = documents(&gallery("review-board").join("hive.json"))
        .err()
        .map(|error| error.to_string());
    assert!(error.is_some_and(|error| error.contains("is not a package directory")));
}

#[test]
fn every_gallery_package_loads_and_keeps_its_sidecars_out_of_the_package() -> Result<()> {
    for name in [
        "review-board",
        "research-debate",
        "incident-response",
        "memory-guild",
        "self-improving",
        "math-panel",
        "math-swarm",
    ] {
        let loaded = load(&gallery(name))?;
        assert_eq!(loaded.lowered.id, name);
        assert!(!loaded.summary.is_empty(), "{name} needs a README summary");
        assert!(!loaded.package.documents.contains_key("sim.json"));
        assert!(!loaded.package.documents.contains_key("script.json"));
    }
    Ok(())
}

#[test]
fn a_summary_is_the_first_paragraph_after_the_heading() {
    assert_eq!(
        summary("# title\n\nOne line\nand two.\n\nNot this."),
        "One line and two."
    );
    assert_eq!(summary(""), "");
}

#[test]
fn the_mermaid_view_has_an_edge_for_every_topology_link() -> Result<()> {
    let loaded = load(&gallery("incident-response"))?;
    let chart = mermaid(&loaded.lowered);
    assert!(chart.starts_with("flowchart LR\n"));
    assert_ne!((loaded.lowered.topology).len(), 0);
    for link in &loaded.lowered.topology {
        let edge = format!("  {} --> {}\n", node(&link.from), node(&link.to));
        assert!(chart.contains(&edge), "missing {edge:?} in\n{chart}");
    }
    for link in &loaded.lowered.referrals {
        let edge = format!("  {} -. refer .-> {}\n", node(&link.from), node(&link.to));
        assert!(chart.contains(&edge), "missing {edge:?}");
    }
    for desk in &loaded.lowered.desks {
        assert!(chart.contains(&format!("subgraph {}[", node(&desk.id))));
    }
    Ok(())
}

#[test]
fn the_problem_bank_answers_are_computed_exactly() -> Result<()> {
    let loaded = load(&gallery("math-swarm"))?;
    let bank = problems(loaded.sim.as_ref().ok_or("math-swarm has a sim.json")?)?;
    let expected = [
        ("lattice-6x6", 924),
        ("lattice-5x8", 1287),
        ("modsum-squares", 14),
        ("modsum-cubes", 30),
        ("digitsum-2pow50", 76),
        ("multiples-3-5", 233_168),
        ("sqdiff-20", 41_230),
        ("primes-200", 46),
    ];
    for (id, answer) in expected {
        let problem = bank.iter().find(|problem| problem.id == id).ok_or(id)?;
        assert_eq!(problem.answer()?, answer, "{id}");
        assert_ne!(problem.classic_mistake()?, answer, "{id}");
    }
    Ok(())
}

#[test]
fn the_structure_reports_the_policy_each_desk_runs_under() -> Result<()> {
    let swarm = crate::structure::structure(&load(&gallery("math-swarm"))?)?;
    let desks = swarm["desks"].as_array().ok_or("desks")?;
    assert_eq!(desks.len(), 5);
    for desk in desks {
        assert_eq!(
            desk["episode"],
            serde_json::json!({"turn_budget":30,"round_width":10,"revealed_width":10,"quorum_threshold":6})
        );
    }
    let panel = crate::structure::structure(&load(&gallery("math-panel"))?)?;
    assert_eq!(panel["desks"][0]["episode"]["quorum_threshold"], 3);
    assert_eq!(panel["desks"][0]["episode"]["turn_budget"], 15);
    Ok(())
}

#[test]
fn the_problem_bank_rejects_zero_divisors_instead_of_panicking() {
    for (kind, args) in [
        ("multiples", serde_json::json!([10, 0, 5])),
        ("multiples", serde_json::json!([10, 3, 0])),
        ("modsum", serde_json::json!([4, 2, 0])),
    ] {
        let sim = serde_json::json!({"problems": [
            {"id": "bad", "kind": kind, "args": args, "statement": "s"}
        ]});
        let error = problems(&sim).err().map(|error| error.to_string());
        assert!(error.is_some_and(|error| error.contains("zero")), "{kind}");
    }
}

#[test]
fn the_problem_bank_rejects_zero_and_overflowing_arguments_instead_of_panicking() {
    for (kind, args) in [
        ("lattice", serde_json::json!([0, 5])),
        ("lattice", serde_json::json!([5, 0])),
        ("lattice", serde_json::json!([100, 100])),
        ("modsum", serde_json::json!([0, 2, 7])),
        ("digitsum", serde_json::json!([2, 0])),
        ("digitsum", serde_json::json!([2, 64])),
        ("digitsum", serde_json::json!([10, 4_294_967_296_u64])),
        ("sqdiff", serde_json::json!([0])),
    ] {
        let sim = serde_json::json!({"problems": [
            {"id": "bad", "kind": kind, "args": args, "statement": "s"}
        ]});
        let rejected = std::panic::catch_unwind(|| problems(&sim).is_err());
        assert_eq!(rejected.ok(), Some(true), "{kind} {args}");
    }
}

#[test]
fn the_problem_bank_still_computes_large_but_fitting_answers() -> Result<()> {
    let sim = serde_json::json!({"problems": [
        {"id": "wide", "kind": "lattice", "args": [33, 33], "statement": "s"},
        {"id": "pow", "kind": "digitsum", "args": [2, 63], "statement": "s"}
    ]});
    let bank = problems(&sim)?;
    assert_eq!(bank[0].answer()?, 7_219_428_434_016_265_740);
    assert_eq!(bank[1].answer()?, 89);
    Ok(())
}

#[cfg(unix)]
#[test]
fn the_loader_refuses_a_symlinked_package_file() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("hive_view-symlink-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let outside = dir.join("outside.txt");
    std::fs::write(&outside, "secret")?;
    let package = dir.join("package");
    std::fs::create_dir_all(&package)?;
    std::fs::write(package.join("hive.json"), "{}")?;
    std::os::unix::fs::symlink(&outside, package.join("README.md"))?;
    let error = crate::load::documents(&package)
        .err()
        .map(|error| error.to_string());
    std::fs::remove_dir_all(&dir)?;
    assert!(error.is_some_and(|error| error.contains("symbolic link")));
    Ok(())
}

#[test]
fn the_problem_bank_refuses_computations_beyond_its_work_limits() {
    for (kind, args) in [
        ("primes", serde_json::json!([100_001])),
        ("multiples", serde_json::json!([1_000_001, 3, 5])),
        ("sqdiff", serde_json::json!([10_001])),
        ("modsum", serde_json::json!([1_001, 1_000, 7])),
        ("modsum", serde_json::json!([1_000_001, 0, 7])),
    ] {
        let sim = serde_json::json!({"problems": [
            {"id": "too-much-work", "kind": kind, "args": args, "statement": "s"}
        ]});
        let error = problems(&sim).err().map(|error| error.to_string());
        assert!(
            error.is_some_and(|error| error.contains("work limit")),
            "{kind} {args}"
        );
    }
}
