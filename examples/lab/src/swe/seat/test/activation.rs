//! One activation: tools, speech, nudges, caps and telemetry.

use serde_json::json;
use tinyhivemind_core::telemetry::TraceEvent;

use super::super::*;
use super::support::*;
use crate::swe::tools::{HIVE_TOOLS, SINGLE_TOOLS};

#[test]
fn bash_then_post_ends_the_activation_with_real_tokens_traced() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "ls" })),
            call("post", json!({ "message": "listed" })),
        ],
        None,
    );
    let out = go(&rig, HIVE_TOOLS, true, 5);
    assert!(out.spoke.is_some() && !out.completed);
    assert_eq!(rig.board.len(), 1);
    let ev = events(&rig);
    assert!(
        ev.iter()
            .any(|e| matches!(e, TraceEvent::Mark { label, .. } if label == "exec"))
    );
    let finished: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            TraceEvent::TurnFinished {
                input_tokens,
                output_tokens,
                ..
            } => Some((*input_tokens, *output_tokens)),
            _ => None,
        })
        .collect();
    assert_eq!(finished, [(100, 10), (100, 10)]);
}

#[test]
fn complete_episode_marks_completion() {
    let rig = rig(
        vec![call("complete_episode", json!({ "message": "done" }))],
        None,
    );
    let out = go(&rig, SINGLE_TOOLS, false, 5);
    assert!(out.completed);
}

#[test]
fn blocked_bash_is_traced_as_refused_with_a_reason() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "rm -rf /" })),
            call("complete_episode", json!({ "message": "x" })),
        ],
        None,
    );
    go(&rig, SINGLE_TOOLS, false, 5);
    let refused = events(&rig).into_iter().find_map(|e| match e {
        TraceEvent::ToolCall {
            tool,
            refused: true,
            reason,
            ..
        } if tool == "bash" => reason,
        _ => None,
    });
    assert!(refused.expect("refused bash").contains("root"));
}

#[test]
fn unoffered_and_malformed_speech_is_refused_not_committed() {
    let rig = rig(
        vec![
            call("dm", json!({ "to": ["tester"], "message": "psst" })),
            call("post", json!({ "message": "  " })),
            call("post", json!({ "message": "ok" })),
        ],
        None,
    );
    let out = go(&rig, HIVE_TOOLS, true, 5);
    assert_eq!(rig.board.len(), 1);
    assert!(out.spoke.is_some());
    let refusals = events(&rig)
        .iter()
        .filter(|e| matches!(e, TraceEvent::ToolCall { refused: true, .. }))
        .count();
    assert_eq!(refusals, 2);
}

#[test]
fn hive_text_without_a_tool_becomes_a_post() {
    let rig = rig(vec![text("I looked and found nothing")], None);
    let out = go(&rig, HIVE_TOOLS, true, 5);
    assert!(out.spoke.is_some());
    assert!(rig.board.read("lead", 5).contains("found nothing"));
}

#[test]
fn single_text_without_a_tool_is_nudged_then_given_up() {
    let rig = rig(vec![text("a"), text("b"), text("c")], None);
    let out = go(&rig, SINGLE_TOOLS, false, 9);
    assert!(out.spoke.is_none() && !out.completed);
    assert_eq!(out.steps, 3);
}

#[test]
fn text_only_replies_are_nudged_only_when_consecutive() {
    let bash = || call("bash", json!({ "cmd": "ls" }));
    let done = call("complete_episode", json!({ "message": "ok" }));
    let rig = rig(
        vec![
            text("thinking"),
            bash(),
            text("thinking"),
            bash(),
            text("thinking"),
            bash(),
            text("thinking"),
            done,
        ],
        None,
    );
    let out = go(&rig, SINGLE_TOOLS, false, 20);
    assert!(
        out.completed,
        "a text-only reply between tool calls must not end the run"
    );
}

#[test]
fn token_cap_aborts_cleanly_mid_activation() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "ls" })),
            call("bash", json!({ "cmd": "ls" })),
        ],
        Some(100),
    );
    let out = go(&rig, SINGLE_TOOLS, false, 9);
    assert!(matches!(out.abort, Some(Abort::TokenCap { .. })));
    assert_eq!(out.steps, 1);
}

#[test]
fn running_out_of_steps_posts_a_stop_note_in_hive_mode() {
    let rig = rig(
        vec![
            call("bash", json!({ "cmd": "a" })),
            call("bash", json!({ "cmd": "b" })),
        ],
        None,
    );
    let out = go(&rig, HIVE_TOOLS, true, 2);
    assert!(out.spoke.is_some());
    assert!(rig.board.read("lead", 5).contains("stopped after 2 steps"));
}

