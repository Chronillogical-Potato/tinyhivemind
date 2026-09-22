//! The seam's pure pieces: which runner the environment names.

use super::RunnerKind;

#[test]
fn the_runner_is_named_by_the_environment_and_defaults_to_embed() {
    // Not set, empty, and each spelling -- read through the same parser the
    // binary uses, without touching the process environment.
    assert_eq!(RunnerKind::parse(None), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("")), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("embed")), Ok(RunnerKind::Embed));
    assert_eq!(RunnerKind::parse(Some("raw")), Ok(RunnerKind::Raw));
    assert!(
        RunnerKind::parse(Some("rae")).is_err(),
        "a typo must not run the default"
    );
}

#[test]
fn each_runner_states_its_own_mechanics_and_nothing_else() {
    assert!(RunnerKind::Embed.how_to_call().contains("mcp_call_tool"));
    assert!(!RunnerKind::Raw.how_to_call().contains("mcp"));
}
