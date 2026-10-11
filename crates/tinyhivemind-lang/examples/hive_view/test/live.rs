//! The live host offline: the request body it would send, the models it
//! refuses, the prompt a seat sees, and how a reply becomes a line. Nothing
//! here makes a call.

use std::collections::BTreeMap;

use serde_json::json;
use tinyhivemind_core::hive::{
    BidReason, HiveTurn, Phase, Sequence, SessionAuthor, Visibility, project_for,
};

use crate::episode::{message, speech};
use crate::live::prompt::{self, Brief, Parsed, parse};
use crate::live::provider::{
    DEFAULT_MODEL, FORBIDDEN_KEYS, artifacts_root_in, check_model, main_checkout, redact_user, request_body,
};

#[test]
fn the_request_body_pins_its_form_and_carries_no_tools_plugins_or_web_search() {
    let body = request_body("openai/gpt-oss-120b:nitro", "sys", "user", 600, 42);
    assert_eq!(
        body,
        json!({
            "model": "openai/gpt-oss-120b:nitro",
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "user"},
            ],
            "max_tokens": 600,
            "temperature": 0.7,
            "seed": 42,
            "reasoning": {"effort": "low", "exclude": true},
            "usage": {"include": true},
        })
    );
    for key in FORBIDDEN_KEYS {
        assert!(body.get(key).is_none(), "{key} must never be sent");
    }
    for key in ["tools", "plugins", "web_search_options"] {
        assert!(FORBIDDEN_KEYS.contains(&key));
    }
}

#[test]
fn a_web_capable_or_malformed_model_is_refused_before_any_call() {
    for refused in [
        "openai/gpt-4o-mini:online",
        "openai/gpt-4o-mini:ONLINE",
        "perplexity/sonar",
        "openai/gpt-4o-search-preview",
        "vendor/model-web",
        "",
        "a b",
        "vendor/model:x:y",
        "-vendor/model",
        "vendor/model;rm",
    ] {
        assert!(
            check_model(refused).is_err(),
            "{refused:?} should be refused"
        );
    }
    for accepted in [
        DEFAULT_MODEL,
        "google/gemini-2.0-flash-001",
        "meta-llama/llama-3.3-70b-instruct:free",
    ] {
        assert!(
            check_model(accepted).is_ok(),
            "{accepted:?} should be accepted"
        );
    }
}

#[test]
fn a_reply_reduces_to_its_first_valid_trace_line() {
    let parsed =
        parse("Let me think.\n\n- **!propose #1,287. ^1 Choose 5 of 13.**\n!support #9 ^2");
    assert_eq!(
        parsed,
        Parsed {
            content: "!propose #1287 ^1 Choose 5 of 13.".into(),
            valid: true,
        }
    );
    // A trace line missing what its kind needs is skipped for the next one.
    let parsed = parse("!propose ^1 no topic\n!object >4 ^3 4 is wrong");
    assert_eq!(parsed.content, "!object >4 ^3 4 is wrong");
    // A referral line rides along.
    let parsed = parse("```\n!evidence ^1 residue 2 mod 3\n```\n@#verify please check");
    assert_eq!(
        parsed.content,
        "!evidence ^1 residue 2 mod 3\n@#verify please check"
    );
    assert!(parsed.valid);
}

#[test]
fn a_reply_without_a_valid_trace_line_is_recorded_as_a_note_that_cannot_vote() {
    let parsed = parse("I believe the answer is 924 because of symmetry.");
    assert!(!parsed.valid);
    assert_eq!(
        parsed.content,
        "note: I believe the answer is 924 because of symmetry."
    );
    let parsed = parse("!vote #924");
    assert!(!parsed.valid);
    assert!(parsed.content.starts_with("note: vote"));
    let row = speech(3, "seat", &parsed.content);
    assert!(tinyhivemind_core::hive::trace::read(std::slice::from_ref(&row)).is_empty());
    assert_eq!(parse("").content, "note: (empty reply)");
}

fn brief() -> Brief {
    Brief {
        seat: "verify-01".into(),
        persona: "You are a \"skeptical auditor\".\nYou trust nothing.".into(),
        role: "verifier".into(),
        charter: "Check every proposal.".into(),
        desk: "verify".into(),
        desk_name: "Verify".into(),
        desk_description: "Independent checkers".into(),
        links: vec!["synthesis".into()],
    }
}

#[test]
fn the_system_prompt_carries_persona_charter_links_grammar_and_no_web() {
    let system = prompt::system(&brief());
    for needle in [
        "You are a \"skeptical auditor\".",
        "Role (verifier): Check every proposal.",
        "Verify desk (verify)",
        "@#synthesis",
        "no web or internet access",
        "!propose #<answer> ^<n>",
        "!object ><n> ^<m>",
        "!commit #<answer>",
    ] {
        assert!(system.contains(needle), "missing {needle:?} in\n{system}");
    }
}

#[test]
fn a_blind_turn_prompt_shows_only_what_core_projects() {
    let journal = vec![
        message(1, SessionAuthor::Operator, "Problem p: 2+2?"),
        speech(2, "compute-01", "!propose #4 ^1 from compute"),
        speech(3, "verify-02", "!propose #5 ^1 a peer this episode"),
    ];
    let turn = HiveTurn {
        agent_id: "verify-01".into(),
        phase: Phase::Deliberate,
        visibility: Visibility::Blind,
        reason: BidReason::Salience,
        watermark: Sequence(2),
        round_start: Sequence(3),
    };
    let visible = project_for(&turn, &journal);
    let home = BTreeMap::from([
        ("compute-01".to_owned(), "compute".to_owned()),
        ("verify-02".to_owned(), "verify".to_owned()),
    ]);
    let text = prompt::turn(&brief(), &turn, &visible, &home, None);
    assert!(text.contains("^1 problem: Problem p: 2+2?"));
    assert!(text.contains("^2 compute-01 (referral from desk compute): !propose #4"));
    assert!(
        !text.contains("a peer this episode"),
        "blind rounds stay blind:\n{text}"
    );
    assert!(text.contains("blind opening round"));
    let commit = HiveTurn {
        phase: Phase::Commit,
        visibility: Visibility::Full,
        ..turn
    };
    let text = prompt::turn(&brief(), &commit, &visible, &home, Some("4"));
    assert!(text.contains("`!commit #4 <reason>`"));
}

#[test]
fn an_error_body_has_its_account_id_redacted() {
    let body = r#"{"error":{"message":"bad","code":400},"user_id":"user_abc123"}"#;
    assert_eq!(
        redact_user(body),
        r#"{"error":{"message":"bad","code":400},"user_id":"[redacted]"}"#
    );
    assert_eq!(redact_user("plain"), "plain");
}

#[test]
fn live_artifacts_live_outside_target_unless_hive_runs_dir_says_otherwise() {
    let workspace = std::path::Path::new("/repo");
    let default = artifacts_root_in(workspace, None);
    assert_eq!(default, workspace.join("hive-runs/hive-view"));
    assert!(!default.starts_with(workspace.join("target")));
    assert_eq!(
        artifacts_root_in(workspace, Some("".into())),
        workspace.join("hive-runs/hive-view")
    );
    assert_eq!(
        artifacts_root_in(workspace, Some("/elsewhere/runs".into())),
        std::path::PathBuf::from("/elsewhere/runs")
    );
}

/// A scratch directory under `target/`, removed when dropped.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/hive-view-test")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(std::fs::canonicalize(&dir).expect("scratch dir resolves"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_linked_worktree_keeps_live_artifacts_in_its_main_checkout() {
    let scratch = Scratch::new("worktree");
    let main = scratch.0.join("main");
    let gitdir = main.join(".git/worktrees/feature");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    let worktree = main.join("worktrees/feature");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();

    assert_eq!(main_checkout(&worktree), main);
    assert_eq!(main_checkout(&main), main, "a main checkout is its own");
    assert_eq!(
        artifacts_root_in(&main_checkout(&worktree), None),
        main.join("hive-runs/hive-view")
    );
}

#[test]
fn a_submodule_or_a_bare_directory_is_its_own_checkout() {
    let scratch = Scratch::new("submodule");
    let parent = scratch.0.join("super");
    let module_gitdir = parent.join(".git/modules/child");
    std::fs::create_dir_all(&module_gitdir).unwrap();
    let child = parent.join("child");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(child.join(".git"), "gitdir: ../.git/modules/child\n").unwrap();
    assert_eq!(main_checkout(&child), child, "no commondir: not a worktree");

    let plain = scratch.0.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(main_checkout(&plain), plain, "no .git at all");
}
