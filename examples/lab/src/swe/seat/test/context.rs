//! Compaction under each context policy.

use serde_json::json;

use super::super::*;
use super::support::*;
use crate::swe::context::Policy;
use crate::swe::tools::SINGLE_TOOLS;


#[test]
fn mask_stubs_old_results_once_the_prompt_passes_the_budget() {
    let (rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        long_bash(),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let out = go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::Mask));
    assert!(out.completed);
    assert_eq!(out.max_prompt, 100);
    assert_eq!(rig.llm.meter().snapshot().max_prompt, 100);
    assert!(context_marks(&rig) >= 1);
    let last = bodies.lock().expect("lock").pop().expect("a request");
    let tools: Vec<&str> = last["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().expect("text"))
        .collect();
    assert_eq!(tools.len(), 3);
    assert!(tools[0].starts_with("[output elided:") && tools[1].starts_with("[output elided:"));
    assert!(
        tools[2].starts_with("exit=0"),
        "newest stays whole: {}",
        tools[2]
    );
}

#[test]
fn none_never_rewrites_the_conversation() {
    let (rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::None));
    assert_eq!(context_marks(&rig), 0);
    let last = bodies.lock().expect("lock").pop().expect("a request");
    assert!(!last.to_string().contains("output elided"));
}

#[test]
fn summarize_spends_one_metered_call_and_replaces_the_oldest_messages() {
    let (rig, bodies) = recorded(vec![
        long_bash(),
        long_bash(),
        Ok(text("NOTE: wrote the file")),
        long_bash(),
        Ok(text("NOTE: wrote the file")),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let out = go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::Summarize));
    assert!(out.completed);
    let snap = rig.llm.meter().snapshot();
    assert_eq!((snap.calls, snap.context_events), (6, 2));
    assert_eq!(context_marks(&rig), 2);
    let last = bodies.lock().expect("lock").pop().expect("a request");
    let messages = last["messages"].as_array().expect("messages");
    assert_eq!(messages[1]["content"], "go");
    assert!(
        messages[2]["content"]
            .as_str()
            .expect("text")
            .contains("NOTE: wrote the file")
    );
    let calls = messages
        .iter()
        .filter(|m| m["tool_calls"].is_array())
        .count();
    let results = messages.iter().filter(|m| m["role"] == "tool").count();
    assert_eq!(calls, results);
}

#[test]
fn a_failed_summary_leaves_the_conversation_alone() {
    let (rig, _) = recorded(vec![
        long_bash(),
        long_bash(),
        Err("boom".into()),
        Err("boom".into()),
        Ok(call("complete_episode", json!({ "message": "done" }))),
    ]);
    let out = go_with(&rig, SINGLE_TOOLS, false, 10, settings(Policy::Summarize));
    assert!(out.abort.is_some() || out.completed);
    assert_eq!(context_marks(&rig), 0);
}

#[test]
fn a_context_overflow_error_aborts_without_a_retry() {
    let (rig, bodies) = recorded(vec![Err(
        "http 400: This model's maximum context length is 131072 tokens".into(),
    )]);
    let out = go(&rig, SINGLE_TOOLS, false, 5);
    assert!(matches!(out.abort, Some(Abort::ContextOverflow(_))));
    assert_eq!(bodies.lock().expect("lock").len(), 1);
    assert_eq!(out.abort.expect("abort").to_string(), "context_overflow");
}

