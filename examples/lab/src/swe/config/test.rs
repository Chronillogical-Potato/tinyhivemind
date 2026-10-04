//! Flag parsing.

use super::*;
use crate::swe::context::Policy;
use crate::swe::session::SessionMode;

fn parse(line: &str) -> Result<Config, UsageError> {
    Config::parse(line.split_whitespace().map(str::to_owned))
}

#[test]
fn parses_a_container_run_with_defaults() {
    let c = parse("--mode single --task fix --container box").expect("parses");
    assert_eq!(c.mode, Mode::Single);
    assert_eq!(c.target, Target::Container("box".into()));
    assert_eq!(c.model, "openai/gpt-oss-120b:nitro");
    assert_eq!((c.max_turns, c.round_width, c.token_cap), (60, 2, None));
}

#[test]
fn context_flags_default_to_masking_at_sixty_thousand() {
    let c = parse("--mode single --task fix --container box").expect("parses");
    assert_eq!(c.single_context, Policy::Mask);
    assert_eq!((c.context_budget, c.context_keep), (60_000, 8));
    assert_eq!(c.hive_settings().policy, Policy::MaskThenSummarize);
}

#[test]
fn parses_the_context_flags_and_rejects_a_bad_policy() {
    let c = parse(
        "--mode single --task x --stdio-rpc --single-context summarize --context-budget 900 --context-keep 3",
    )
    .expect("parses");
    assert_eq!(c.single_settings().policy, Policy::Summarize);
    assert_eq!((c.context_budget, c.context_keep), (900, 3));
    assert!(parse("--mode single --task x --stdio-rpc --single-context nope").is_err());
}

#[test]
fn parses_every_numeric_flag() {
    let c = parse(
        "--mode hive --task x --stdio-rpc --max-turns 9 --round-width 3 --token-cap 5000 \
         --steps-per-turn 4 --trace t.jsonl --result r.json",
    )
    .expect("parses");
    assert_eq!(c.target, Target::StdioRpc);
    assert_eq!(
        (c.max_turns, c.round_width, c.token_cap, c.steps_per_turn),
        (9, 3, Some(5000), 4)
    );
    assert!(c.trace.is_some() && c.result.is_some());
}

#[test]
fn rejects_missing_or_conflicting_input() {
    assert!(parse("--task x --container b").is_err());
    assert!(parse("--mode hive --container b").is_err());
    assert!(parse("--mode hive --task x").is_err());
    assert!(parse("--mode hive --task x --container b --stdio-rpc").is_err());
    assert!(parse("--mode swarm --task x --container b").is_err());
    assert!(parse("--mode hive --task x --container b --max-turns abc").is_err());
    assert!(parse("--mode hive --task x --container b --max-turns 0").is_err());
    assert!(parse("--mode hive --task x --container b --bogus 1").is_err());
    assert!(parse("--mode hive --task").is_err());
}

#[test]
fn reads_a_task_file() {
    let path = std::env::temp_dir().join(format!("swe_task_{}.txt", std::process::id()));
    std::fs::write(&path, "do the thing").expect("write");
    let c = Config::parse(
        [
            "--mode",
            "single",
            "--task-file",
            path.to_str().expect("utf8"),
            "--container",
            "b",
        ]
        .map(str::to_owned),
    )
    .expect("parses");
    std::fs::remove_file(&path).ok();
    assert_eq!(c.task, "do the thing");
}

#[test]
fn sessions_persist_by_default_and_fresh_restores_masking() {
    let c = parse("--mode hive --task x --stdio-rpc").expect("parses");
    assert_eq!(c.seat_session, SessionMode::Persistent);
    assert_eq!(c.hive_settings().policy, Policy::MaskThenSummarize);
    let fresh = parse("--mode hive --task x --stdio-rpc --seat-session fresh").expect("parses");
    assert_eq!(fresh.seat_session, SessionMode::Fresh);
    assert_eq!(fresh.hive_settings().policy, Policy::Mask);
    assert!(parse("--mode hive --task x --stdio-rpc --seat-session sticky").is_err());
}

#[test]
fn hive_context_overrides_the_session_default() {
    let c = parse("--mode hive --task x --stdio-rpc --hive-context mask").expect("parses");
    assert_eq!(c.hive_settings().policy, Policy::Mask);
    let c = parse("--mode hive --task x --stdio-rpc --seat-session fresh --hive-context summarize")
        .expect("parses");
    assert_eq!(c.hive_settings().policy, Policy::MaskThenSummarize);
    assert!(parse("--mode hive --task x --stdio-rpc --hive-context none").is_err());
}

#[test]
fn memory_flags_parse_with_defaults() {
    let c = parse("--mode hive --task x --stdio-rpc").expect("parses");
    assert_eq!(
        (c.memory, c.memory_url.as_deref(), c.memory_budget, c.run_id.as_deref()),
        (MemoryKind::None, None, 1200, None)
    );
    let c = parse(
        "--mode single --task x --stdio-rpc --memory cortex --memory-url http://m:1 \
         --memory-budget 300 --run-id trial-7",
    )
    .expect("parses");
    assert_eq!(c.memory, MemoryKind::Cortex);
    assert_eq!(c.memory_url.as_deref(), Some("http://m:1"));
    assert_eq!((c.memory_budget, c.run_id.as_deref()), (300, Some("trial-7")));
    assert_eq!(c.memory.name(), "cortex");
    assert!(parse("--mode hive --task x --stdio-rpc --memory redis").is_err());
    assert!(parse("--mode hive --task x --stdio-rpc --memory-budget 0").is_err());
}
