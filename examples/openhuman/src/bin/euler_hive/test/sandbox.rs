//! The sandbox argv and output bound, the spend budget, and search refusals.

use std::io::Read;

use super::super::sandbox::docker_args;
use super::super::search::condense_exa;

#[test]
fn the_sandbox_has_no_network_no_mounts_and_bounded_resources() {
    let args = docker_args("python:3.12-slim", "euler-test");
    let pair = |flag: &str| {
        args.iter()
            .position(|arg| arg == flag)
            .map(|index| args[index + 1].as_str())
    };
    assert_eq!(pair("--network"), Some("none"));
    assert_eq!(pair("--memory"), Some("1g"));
    assert_eq!(pair("--pids-limit"), Some("128"));
    assert_eq!(pair("--cap-drop"), Some("ALL"));
    assert_eq!(pair("--security-opt"), Some("no-new-privileges"));
    assert_eq!(pair("--user"), Some("65534:65534"));
    assert!(args.contains(&"--read-only".to_owned()));
    assert!(args.contains(&"--rm".to_owned()));
    for mount in [
        "--mount",
        "-v",
        "--volume",
        "--volumes-from",
        "--privileged",
    ] {
        assert!(!args.iter().any(|arg| arg == mount), "{mount} present");
    }
    assert!(!args.iter().any(|arg| arg.contains("hive-runs")));
    let tail: Vec<_> = args.iter().rev().take(3).collect();
    assert_eq!(tail, ["-", "-I", "python3"]);
}

#[test]
fn exa_results_condense_to_numbered_sources() {
    let found = condense_exa(&serde_json::json!({"results": [
        {"title": "Witt design", "url": "https://example.org/w", "text": "a design"},
        {"title": "No url"}
    ]}));
    assert!(
        found
            .text
            .starts_with("[1] Witt design\nhttps://example.org/w\na design")
    );
    assert_eq!(found.urls, ["https://example.org/w"]);
    assert_eq!(condense_exa(&serde_json::json!({})).text, "no results");
}

#[test]
fn searches_that_quote_the_statement_or_look_the_problem_up_are_refused() {
    use super::super::search::{refused_query, statement_literals};
    let statement = "Rows: QWERTYUI and ZXCVBNMA. Given T(10) = 3.1415926535 and n = 24.";
    let literals = statement_literals(statement);
    assert_eq!(literals, ["3.1415926535", "QWERTYUI", "ZXCVBNMA"]);
    assert!(refused_query("\"QWERTYUI\" octad", &literals).is_some());
    assert!(refused_query("sum equal to 3.1415926535", &literals).is_some());
    assert!(refused_query("Project Euler rock paper scissors", &literals).is_some());
    assert!(refused_query("Witt design S(5,8,24) uniqueness", &literals).is_none());
    assert!(refused_query("Nash equilibrium skew-symmetric game support", &literals).is_none());
}

#[test]
fn a_third_of_the_sandbox_budget_is_held_back_for_the_verify_desk() {
    let mut limits = super::super::budget::Limits::SMOKE;
    limits.max_sandbox_runs = 6;
    let budget = super::super::budget::Budget::new(limits);
    for _ in 0..4 {
        budget
            .claim_sandbox("compute")
            .expect("compute may use its share");
    }
    let refused = budget
        .claim_sandbox("compute")
        .expect_err("compute is out of its share");
    assert!(refused.to_string().contains("verify"), "{refused}");
    budget
        .claim_sandbox("verify")
        .expect("verify keeps its reserve");
    budget
        .claim_sandbox("verify")
        .expect("verify keeps its reserve");
    assert!(
        budget.claim_sandbox("verify").is_err(),
        "the total still binds"
    );
}

#[test]
fn a_spend_cap_must_be_finite_and_non_negative() {
    assert_eq!(super::super::spend_cap("2.5").expect("a real cap"), 2.5);
    for bad in ["NaN", "inf", "-inf", "-1"] {
        assert!(super::super::spend_cap(bad).is_err(), "{bad} accepted");
    }
}

#[test]
fn concurrent_claims_never_take_the_verify_reserve() {
    let mut limits = super::super::budget::Limits::SMOKE;
    limits.max_sandbox_runs = 30;
    let budget = std::sync::Arc::new(super::super::budget::Budget::new(limits));
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let budget = budget.clone();
            std::thread::spawn(move || {
                (0..10)
                    .filter(|_| budget.claim_sandbox("compute").is_ok())
                    .count()
            })
        })
        .collect();
    let granted: usize = threads
        .into_iter()
        .map(|thread| thread.join().expect("claimant"))
        .sum();
    assert_eq!(granted, 20, "compute gets exactly its two-thirds share");
    for _ in 0..10 {
        budget
            .claim_sandbox("verify")
            .expect("the reserve is intact");
    }
    assert!(budget.claim_sandbox("verify").is_err());
}

#[test]
fn sandbox_output_is_bounded_while_it_is_read() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let flood = std::io::repeat(b'x').take(10 * super::super::sandbox::MAX_OUTPUT_BYTES as u64);
    let overflow = AtomicBool::new(false);
    let kept = super::super::sandbox::read_bounded(
        flood,
        super::super::sandbox::MAX_OUTPUT_BYTES,
        &overflow,
    );
    assert_eq!(kept.len(), super::super::sandbox::MAX_OUTPUT_BYTES);
    assert!(
        overflow.load(Ordering::Acquire),
        "a flood stops the program"
    );
    let quiet = AtomicBool::new(false);
    let kept = super::super::sandbox::read_bounded(
        &b"small"[..],
        super::super::sandbox::MAX_OUTPUT_BYTES,
        &quiet,
    );
    assert_eq!(kept, b"small");
    assert!(!quiet.load(Ordering::Acquire));
    let args = docker_args("python:3.12-slim", "euler-test");
    let at = args
        .iter()
        .position(|arg| arg == "--log-driver")
        .expect("log driver set");
    assert_eq!(args[at + 1], "none", "Docker keeps no copy of the output");
}
