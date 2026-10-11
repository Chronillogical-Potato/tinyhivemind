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

#[test]
fn live_flags_parse_with_bounds_and_refuse_a_web_model() {
    let parse = |argv: &[&str]| crate::parse_args(argv.iter().map(|arg| (*arg).to_owned()));
    let args = parse(&["dir", "--run", "--live", "--max-calls", "40", "--max-tokens", "512"]);
    assert!(args.is_ok_and(|args| args.live && args.max_calls == 40 && args.max_tokens == 512));
    assert!(parse(&["dir", "--live", "--max-calls", "0"]).is_err());
    assert!(parse(&["dir", "--live", "--max-calls", "2001"]).is_err());
    assert!(parse(&["dir", "--live", "--max-tokens", "10"]).is_err());
    assert!(parse(&["dir", "--live", "--model", "openai/gpt-4o:online"]).is_err());
    assert!(parse(&["dir", "--live", "--model", "a b"]).is_err());
}

#[test]
fn the_live_model_comes_from_the_flag_then_the_environment_then_the_default()
-> crate::Result<()> {
    let parse = |argv: &[&str]| crate::parse_args(argv.iter().map(|arg| (*arg).to_owned()));
    let offline = parse(&["dir", "--run"])?;
    assert_eq!(offline.live_config(Some("x/y".into()))?, None);
    let live = parse(&["dir", "--run", "--live"])?;
    let model = |env: Option<&str>| -> crate::Result<String> {
        Ok(live
            .live_config(env.map(str::to_owned))?
            .ok_or("live")?
            .model)
    };
    assert_eq!(model(None)?, crate::live::provider::DEFAULT_MODEL);
    assert_eq!(model(Some(""))?, crate::live::provider::DEFAULT_MODEL);
    assert_eq!(model(Some("google/gemini-2.0-flash-001"))?, "google/gemini-2.0-flash-001");
    // A web variant from the environment is refused before any call.
    assert!(live.live_config(Some("openai/gpt-4o-mini:online".into())).is_err());
    let flagged = parse(&["dir", "--run", "--live", "--model", "meta-llama/llama-3.3-70b-instruct"])?;
    assert_eq!(
        flagged.live_config(Some("x/y".into()))?.ok_or("live")?.model,
        "meta-llama/llama-3.3-70b-instruct"
    );
    Ok(())
}
