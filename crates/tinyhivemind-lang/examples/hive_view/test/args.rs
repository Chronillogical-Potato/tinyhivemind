//! Command-line parsing: knob bounds and the `--live` flags.

#[test]
fn knobs_outside_their_bounds_are_rejected() {
    let parse = |argv: &[&str]| crate::parse_args(argv.iter().map(|arg| (*arg).to_owned()));
    assert!(parse(&["dir", "--round-width", "0"]).is_err());
    assert!(parse(&["dir", "--turn-budget", "2001"]).is_err());
    assert!(parse(&["dir", "--seed", "x"]).is_err());
    assert!(parse(&["dir", "--bogus"]).is_err());
    assert!(
        parse(&["dir", "--round-width", "8", "--seed", "4"])
            .is_ok_and(|args| args.knobs.seed == 4 && args.knobs.round_width == Some(8))
    );
}
