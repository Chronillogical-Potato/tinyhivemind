//! Masking and summary-cut behavior on hand-built conversations.

use serde_json::{Value, json};

use super::*;

fn call(id: &str, cmd: &str) -> Value {
    json!({ "role": "assistant", "content": "", "tool_calls": [
        { "id": id, "type": "function",
          "function": { "name": "bash", "arguments": json!({ "cmd": cmd }).to_string() } }] })
}

fn result(id: &str, body: &str) -> Value {
    json!({ "role": "tool", "tool_call_id": id, "content": body })
}

/// system, user, then `n` bash exchanges with 500-byte outputs.
fn session(n: usize) -> Vec<Value> {
    let mut m = vec![
        json!({ "role": "system", "content": "sys" }),
        json!({ "role": "user", "content": "do the task" }),
    ];
    for i in 0..n {
        m.push(call(&format!("c{i}"), &format!("echo {i}")));
        m.push(result(&format!("c{i}"), &"x".repeat(500)));
    }
    m
}

fn tool_bodies(m: &[Value]) -> Vec<String> {
    m.iter()
        .filter(|x| x["role"] == "tool")
        .map(|x| x["content"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn keeps_the_recent_results_and_stubs_the_older() {
    let mut m = session(12);
    assert_eq!(mask_observations(&mut m, 8), 4);
    let bodies = tool_bodies(&m);
    for old in &bodies[..4] {
        assert!(old.len() < 100, "{old}");
        assert!(
            old.starts_with("[output elided: 500 bytes, cmd=echo "),
            "{old}"
        );
    }
    for recent in &bodies[4..] {
        assert_eq!(recent.len(), 500);
    }
}

#[test]
fn masking_twice_changes_nothing_more() {
    let mut m = session(12);
    mask_observations(&mut m, 8);
    let once = m.clone();
    assert_eq!(mask_observations(&mut m, 8), 0);
    assert_eq!(m, once);
}

#[test]
fn never_touches_the_first_messages_or_assistant_text() {
    let mut m = session(12);
    let before = m.clone();
    mask_observations(&mut m, 0);
    assert_eq!(m[0], before[0]);
    assert_eq!(m[1], before[1]);
    for (a, b) in m
        .iter()
        .zip(&before)
        .filter(|(a, _)| a["role"] == "assistant")
    {
        assert_eq!(a, b);
    }
    assert_eq!(m.len(), before.len());
}

#[test]
fn every_tool_call_keeps_its_tool_message() {
    let mut m = session(12);
    mask_observations(&mut m, 3);
    let ids: Vec<&str> = m
        .iter()
        .filter_map(|x| x["tool_calls"][0]["id"].as_str())
        .collect();
    for id in ids {
        assert_eq!(m.iter().filter(|x| x["tool_call_id"] == id).count(), 1);
    }
}

#[test]
fn short_results_are_not_replaced_by_longer_stubs() {
    let mut m = session(0);
    m.push(call("a", "true"));
    m.push(result("a", "exit=0\n"));
    assert_eq!(mask_observations(&mut m, 0), 0);
    assert_eq!(m[3]["content"], "exit=0\n");
}

#[test]
fn long_commands_are_trimmed_in_the_stub() {
    let mut m = session(0);
    m.push(call("a", &format!("echo {}\nsecond", "y".repeat(300))));
    m.push(result("a", &"z".repeat(900)));
    mask_observations(&mut m, 0);
    let stub = m[3]["content"].as_str().unwrap();
    assert!(
        stub.len() < 160 && !stub.contains('\n') && stub.ends_with("...]"),
        "{stub}"
    );
}

#[test]
fn summary_cut_lands_on_an_assistant_message() {
    let m = session(10);
    let cut = summary_cut(&m).expect("long enough");
    assert_eq!(m[cut]["role"], "assistant");
    assert!(cut > 2 && cut + 1 < m.len());
    assert_eq!(summary_cut(&session(1)), None);
}

#[test]
fn replacing_the_prefix_keeps_pairing_and_the_first_user_message() {
    let mut m = session(10);
    let cut = summary_cut(&m).unwrap();
    let removed = replace_prefix(&mut m, cut, "did things");
    assert_eq!(removed, cut - 2);
    assert_eq!(m[1]["content"], "do the task");
    assert!(m[2]["content"].as_str().unwrap().contains("did things"));
    assert_eq!(m[3]["role"], "assistant");
    for id in m.iter().filter_map(|x| x["tool_calls"][0]["id"].as_str()) {
        assert_eq!(m.iter().filter(|x| x["tool_call_id"] == id).count(), 1);
    }
    assert_eq!(replace_prefix(&mut m, 2, "no"), 0);
    assert_eq!(replace_prefix(&mut m, 999, "no"), 0);
}

#[test]
fn render_includes_commands_and_trims_bodies() {
    let m = session(2);
    let text = render_for_summary(&m[2..]);
    assert!(text.contains("echo 0") && text.contains("tool:"));
    assert!(text.len() < 1200);
}

#[test]
fn policy_names_round_trip() {
    for p in [Policy::None, Policy::Mask, Policy::Summarize] {
        assert_eq!(Policy::parse(p.name()), Some(p));
    }
    assert_eq!(Policy::parse("bogus"), None);
}

#[test]
fn the_layered_policy_parses_from_both_names() {
    assert_eq!(
        Policy::parse("mask+summarize"),
        Some(Policy::MaskThenSummarize)
    );
    assert_eq!(Policy::MaskThenSummarize.name(), "mask+summarize");
}

#[test]
fn the_estimate_scales_the_reported_prompt_by_what_was_removed() {
    assert_eq!(scaled_estimate(1000, 400, 100), 250);
    assert_eq!(scaled_estimate(1000, 0, 0), 1000);
}

#[test]
fn memory_is_inserted_once_after_the_opening_and_then_replaced() {
    let mut m = session(2);
    upsert_memory(&mut m, "first pack");
    assert_eq!(m.len(), 7);
    assert!(m[2]["content"].as_str().unwrap().contains("first pack"));
    upsert_memory(&mut m, "second pack");
    assert_eq!(m.len(), 7, "replaced, not stacked");
    let body = m[2]["content"].as_str().unwrap();
    assert!(body.contains("second pack") && !body.contains("first pack"));
    assert_eq!(m[3]["role"], "assistant", "no tool result is parted");
}

#[test]
fn dropped_text_lists_each_message_once_clipped() {
    let m = session(2);
    let dropped = dropped_text(&m[2..6]);
    assert_eq!(dropped.len(), 4);
    assert!(dropped[0].contains("echo 0"));
    assert!(dropped.iter().all(|line| line.chars().count() <= 403));
}
